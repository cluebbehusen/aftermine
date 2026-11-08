//! Three devices, an iCloud account and iCloud Drive, driven through random
//! saves, deletions, syncs and failures, with iCloud Drive and CloudKit both as
//! unkind as they can be. Whatever happens, once things go quiet every device
//! must hold the same records, the newest save of every record must still
//! exist unless a player deleted it, and a save the player deleted must stay
//! deleted.

use super::tests::{FakeCloud, Game, device, put_file, tv, write_save};
use super::*;
use chassis::rng::Pcg32;

const NAMES: [&str; 7] = [
    "autosaves/autosave-1.json",
    "autosaves/autosave-2.json",
    "autosaves/autosave-3.json",
    "autosaves/autosave-4.json",
    "matches/match-1.json",
    "saves/save-1.json",
    "saves/save-2.json",
];

/// A record's bytes and the save they hold.
#[derive(Clone, PartialEq)]
struct Copy {
    bytes: Vec<u8>,
    ticks: u64,
    saved_at: u64,
}

fn read_copy(path: &Path) -> Option<Copy> {
    let meta = crate::saved_game::inspect(path).ok()?.meta;
    Some(Copy {
        bytes: std::fs::read(path).ok()?,
        ticks: meta.ticks?,
        saved_at: meta.saved_at?,
    })
}

fn stand_in(path: &Path) -> PathBuf {
    let file = path.file_name().unwrap().to_str().unwrap();
    path.with_file_name(format!(".{file}.icloud"))
}

/// The record names a device's folders hold as real files.
fn held(places: &Places) -> Vec<String> {
    places.local().found.into_keys().collect()
}

/// Every record a device holds, by name, with its save time.
fn contents(places: &Places) -> BTreeMap<String, u64> {
    held(places)
        .into_iter()
        .filter_map(|name| {
            let copy = read_copy(&places.path(&name)?)?;
            Some((name, copy.saved_at))
        })
        .collect()
}

/// iCloud Drive between iPad and Mac. It syncs whole files, keeps the newer of
/// two edits unless told otherwise, can leave files on a device as stand-ins or
/// simply not there yet, and can go back to an older copy.
#[derive(Default)]
struct Drive {
    files: BTreeMap<String, Copy>,
    /// The copy each file held before its current one.
    earlier: BTreeMap<String, Copy>,
    seen: [BTreeMap<String, Copy>; 2],
    /// Files each device has lost and iCloud has yet to bring back.
    late: [BTreeSet<String>; 2],
    /// Saves iCloud Drive replaced, which only it may have held.
    replaced: Vec<u64>,
}

impl Drive {
    fn put(&mut self, name: &str, copy: Copy) {
        if let Some(old) = self.files.insert(name.to_owned(), copy) {
            self.replaced.push(old.saved_at);
            self.earlier.insert(name.to_owned(), old);
        }
    }

    /// Syncs one device. With `deliver`, it brings down everything owed.
    fn sync(&mut self, who: usize, places: &Places, deliver: bool) {
        let listing = places.local();
        let local: BTreeMap<String, Copy> = listing
            .found
            .keys()
            .filter_map(|name| Some((name.clone(), read_copy(&places.path(name)?)?)))
            .collect();
        for name in local.keys() {
            self.late[who].remove(name);
        }
        for (name, copy) in &local {
            if self.seen[who].get(name) == Some(copy) {
                continue;
            }
            let other_edit = self
                .files
                .get(name)
                .filter(|other| Some(*other) != self.seen[who].get(name));
            if other_edit.is_none_or(|other| other.saved_at <= copy.saved_at) {
                self.put(name, copy.clone());
            }
        }
        let gone: Vec<String> = self.seen[who]
            .iter()
            .filter(|(name, copy)| {
                !local.contains_key(*name)
                    && !listing.arriving.contains(*name)
                    && !self.late[who].contains(*name)
                    && self.files.get(*name) == Some(*copy)
            })
            .map(|(name, _)| name.clone())
            .collect();
        for name in gone {
            self.files.remove(&name);
        }
        let waiting = |name: &str, late: &BTreeSet<String>| {
            !deliver && (listing.arriving.contains(name) || late.contains(name))
        };
        for (name, copy) in &self.files {
            if waiting(name, &self.late[who]) {
                continue;
            }
            let path = places.path(name).unwrap();
            if local.get(name) != Some(copy) {
                self.replaced
                    .extend(local.get(name).map(|old| old.saved_at));
                put_file(&path, &copy.bytes, copy.saved_at);
            }
            std::fs::remove_file(stand_in(&path)).ok();
            self.seen[who].insert(name.clone(), copy.clone());
        }
        if deliver {
            self.late[who].clear();
        }
        let known: Vec<String> = local
            .keys()
            .chain(listing.arriving.iter())
            .chain(self.seen[who].keys())
            .cloned()
            .collect();
        for name in known {
            if !self.files.contains_key(&name) {
                let path = places.path(&name).unwrap();
                std::fs::remove_file(&path).ok();
                std::fs::remove_file(stand_in(&path)).ok();
                self.seen[who].remove(&name);
                self.late[who].remove(&name);
            }
        }
    }

    /// Swaps a file that has fully reached iCloud for a stand-in, as iOS
    /// does to free space.
    fn evict(&self, places: &Places, name: &str) {
        let path = places.path(name).unwrap();
        if read_copy(&path).is_some_and(|copy| self.files.get(name) == Some(&copy)) {
            std::fs::remove_file(&path).unwrap();
            std::fs::write(stand_in(&path), b"").unwrap();
        }
    }

    /// Takes a file off a device without iCloud treating that as a delete, as
    /// after a restore while the folder is still filling in.
    fn lose(&mut self, who: usize, places: &Places, name: &str) {
        let path = places.path(name).unwrap();
        if read_copy(&path).is_some_and(|copy| self.files.get(name) == Some(&copy))
            && std::fs::remove_file(path).is_ok()
        {
            self.late[who].insert(name.to_owned());
        }
    }

    /// Goes back to the copy before the current one, as resolving a conflict
    /// can. Returns the save it dropped.
    fn regress(&mut self, name: &str) -> Option<u64> {
        let earlier = self.earlier.remove(name)?;
        let current = self.files.insert(name.to_owned(), earlier)?;
        Some(current.saved_at)
    }
}

struct World {
    cloud: FakeCloud,
    /// iPad, Mac, Apple TV.
    devices: [Places; 3],
    games: [Game; 3],
    drive: Drive,
    rng: Pcg32,
    clock: u64,
    /// What each device's last pass found missing.
    missing: [Vec<String>; 3],
    /// The save of each name furthest along: its ticks and its save time, which
    /// tells it from every other save.
    furthest: BTreeMap<String, (u64, u64)>,
    /// The highest delete record a player has asked for under each name.
    deleted_to: BTreeMap<String, u64>,
    /// Saves a player deleted.
    deleted: BTreeSet<u64>,
    /// Saves the game deleted once they had synced, with the name it deleted
    /// them under, which must stay deleted.
    deleted_for_good: BTreeSet<(String, u64)>,
    /// Saves that only tvOS or iCloud Drive held when it dropped them, which
    /// nothing could keep.
    dropped: BTreeSet<u64>,
    /// Whether CloudKit or a device forgot what it agreed. A device starting
    /// over counts its files as new, so a stale copy it then meets can bring a
    /// deleted save back, which loses nothing.
    forgot: bool,
    /// Whether CloudKit was reset. A device already fetching from the start
    /// cannot tell, so what it agreed earlier goes back up only when it next
    /// changes. Only resetting the Development database does this.
    reset: bool,
    rounds: usize,
    log: Vec<String>,
}

const TV: usize = 2;

impl World {
    fn new(seed: u64) -> Self {
        Self {
            cloud: FakeCloud::signed_in(),
            devices: [device("ipad"), device("mac"), tv()],
            games: Default::default(),
            drive: Drive::default(),
            rng: Pcg32::new(seed, 1),
            clock: 1_000_000,
            missing: Default::default(),
            furthest: BTreeMap::new(),
            deleted_to: BTreeMap::new(),
            deleted: BTreeSet::new(),
            deleted_for_good: BTreeSet::new(),
            dropped: BTreeSet::new(),
            forgot: false,
            reset: false,
            rounds: 0,
            log: Vec::new(),
        }
    }

    fn below(&mut self, limit: usize) -> usize {
        self.rng.next_u32() as usize % limit
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn pick_held(&mut self, who: usize) -> Option<String> {
        let names = held(&self.devices[who]);
        if names.is_empty() {
            return None;
        }
        let at = self.below(names.len());
        Some(names[at].clone())
    }

    fn run(&mut self, who: usize, mode: Mode) {
        // A failed pass leaves everything for a later one, as in the app.
        if let Ok(outcome) = pass(&self.cloud, &self.devices[who], mode, &self.games[who]) {
            assert!(
                outcome.missing.iter().all(|name| player_kept(name)),
                "asked about a file the game manages: {:?}",
                outcome.missing
            );
            self.missing[who] = outcome.missing;
        }
    }

    fn sync_drive(&mut self, who: usize, deliver: bool) {
        self.drive.sync(who, &self.devices[who], deliver);
        for saved_at in std::mem::take(&mut self.drive.replaced) {
            if !self.held_anywhere(saved_at, usize::MAX) {
                self.dropped.insert(saved_at);
            }
        }
    }

    fn agreed_file(&self, who: usize, name: &str) -> Option<(u64, u64)> {
        let entry = *load(&self.devices[who].manifest).synced.get(name)?;
        entry.stamp.map(|_| (entry.ticks, entry.saved_at))
    }

    /// Notes that a player deleted `name` on a device.
    fn note_deletion(&mut self, who: usize, name: &str) {
        let path = self.devices[who].path(name).unwrap();
        if let Some(copy) = read_copy(&path) {
            self.deleted.insert(copy.saved_at);
        }
    }

    /// Notes a deletion the mirror will carry: everything up to `ticks`.
    fn note_delete_record(&mut self, name: &str, ticks: u64) {
        let to = self.deleted_to.entry(name.to_owned()).or_default();
        *to = (*to).max(ticks + 1);
    }

    /// Ends the app before a pass took what the player did.
    fn die(&mut self, who: usize) {
        let manifest = load(&self.devices[who].manifest);
        for path in self.games[who].deleted.take() {
            if let Some(name) = self.devices[who].name_of(&path)
                && let Some(entry) = manifest.synced.get(&name)
            {
                self.deleted_for_good.remove(&(name, entry.saved_at));
            }
        }
        for answer in self.games[who].answers.take() {
            if let Answer::Delete(name) = answer
                && let Some(entry) = manifest.synced.get(&name)
            {
                self.deleted_for_good.remove(&(name, entry.saved_at));
            }
        }
    }

    fn held_anywhere(&self, saved_at: u64, except: usize) -> bool {
        self.cloud.holds(saved_at)
            || self.drive.files.values().any(|c| c.saved_at == saved_at)
            || self
                .devices
                .iter()
                .enumerate()
                .filter(|(who, _)| *who != except)
                .any(|(_, places)| contents(places).values().any(|at| *at == saved_at))
    }

    /// Takes a record off Apple TV the way tvOS does.
    fn purge(&mut self, name: &str) {
        let path = self.devices[TV].path(name).unwrap();
        if let Some(copy) = read_copy(&path)
            && !self.held_anywhere(copy.saved_at, TV)
        {
            self.dropped.insert(copy.saved_at);
        }
        std::fs::remove_file(path).ok();
    }

    /// Loses a device's manifest, along with any deletions not yet sent.
    fn lose_manifest(&mut self, who: usize) {
        self.die(who);
        let manifest = load(&self.devices[who].manifest);
        for name in &manifest.deleted {
            if let Some(entry) = manifest.synced.get(name) {
                self.deleted_for_good
                    .remove(&(name.clone(), entry.saved_at));
            }
        }
        std::fs::remove_file(&self.devices[who].manifest).ok();
    }

    /// The player answers a device's question, one way for all its files.
    fn answer(&mut self, who: usize, delete: bool) {
        for name in std::mem::take(&mut self.missing[who]) {
            if delete {
                if let Some((ticks, saved_at)) = self.agreed_file(who, &name) {
                    self.note_delete_record(&name, ticks);
                    self.deleted.insert(saved_at);
                    self.deleted_for_good.insert((name.clone(), saved_at));
                }
                self.games[who]
                    .answers
                    .borrow_mut()
                    .push(Answer::Delete(name));
            } else {
                self.games[who]
                    .answers
                    .borrow_mut()
                    .push(Answer::BringBack(name));
            }
        }
        self.run(who, Mode::Full);
    }

    /// Writes the next save of `name` on a device: on from its own copy, however
    /// old, as resuming a match does, or past anything before it for a new one.
    fn save(&mut self, who: usize, name: &str) -> String {
        let saved_at = self.tick();
        let path = self.devices[who].path(name).unwrap();
        let played = 1 + self.below(50) as u64;
        let base = read_copy(&path).map_or_else(
            || {
                self.furthest
                    .get(name)
                    .map_or(0, |(ticks, _)| *ticks)
                    .max(self.deleted_to.get(name).copied().unwrap_or(0))
            },
            |copy| copy.ticks,
        );
        let ticks = base + played;
        write_save(&path, ticks, saved_at);
        if self
            .furthest
            .get(name)
            .is_none_or(|(most, _)| ticks >= *most)
        {
            self.furthest.insert(name.to_owned(), (ticks, saved_at));
        }
        format!("save {name} at tick {ticks}, time {saved_at}")
    }

    fn step(&mut self) {
        let who = self.below(3);
        let which = self.below(120);
        let op = match which {
            0..=23 => {
                let at = self.below(NAMES.len());
                self.save(who, NAMES[at])
            }
            24..=47 => {
                let open = self.below(5) != 0;
                self.games[who].arrivals.set(open);
                self.run(who, Mode::Full);
                self.games[who].arrivals.set(true);
                "full pass".to_owned()
            }
            48..=59 => {
                self.games[who].arrivals.set(false);
                self.run(who, Mode::Push);
                self.games[who].arrivals.set(true);
                "push during a match".to_owned()
            }
            60..=67 => {
                let Some(name) = self.pick_held(who) else {
                    return;
                };
                self.note_deletion(who, &name);
                let path = self.devices[who].path(&name).unwrap();
                if let (Some(copy), Some((ticks, saved_at))) =
                    (read_copy(&path), self.agreed_file(who, &name))
                    && saved_at == copy.saved_at
                {
                    self.note_delete_record(&name, ticks);
                    self.deleted_for_good.insert((name.clone(), copy.saved_at));
                }
                self.games[who].deleted.borrow_mut().push(path.clone());
                std::fs::remove_file(path).unwrap();
                // Autosave cleanup runs with no pass right behind it.
                match self.below(3) {
                    0 => self.run(who, Mode::Full),
                    1 => self.run(who, Mode::Push),
                    _ => {}
                }
                format!("game delete {name}")
            }
            68..=71 => {
                let who = self.below(TV);
                let Some(name) = self.pick_held(who) else {
                    return;
                };
                self.note_deletion(who, &name);
                std::fs::remove_file(self.devices[who].path(&name).unwrap()).unwrap();
                format!("finder delete {name} on {who}")
            }
            72..=75 => {
                let who = self.below(3);
                if self.missing[who].is_empty() {
                    return;
                }
                let delete = self.below(4) == 0;
                self.answer(who, delete);
                format!("player answers on {who}: delete {delete}")
            }
            76..=78 => {
                let Some(name) = self.pick_held(TV) else {
                    return;
                };
                self.purge(&name);
                format!("tvOS removes {name}")
            }
            79 => {
                for name in held(&self.devices[TV]) {
                    self.purge(&name);
                }
                self.lose_manifest(TV);
                "tvOS clears the cache".to_owned()
            }
            80..=91 => {
                let who = self.below(TV);
                self.sync_drive(who, false);
                format!("drive sync {who}")
            }
            92..=94 => {
                let who = self.below(TV);
                let Some(name) = self.pick_held(who) else {
                    return;
                };
                self.drive.evict(&self.devices[who], &name);
                format!("iOS evicts {name} on {who}")
            }
            95..=97 => {
                let who = self.below(TV);
                let Some(name) = self.pick_held(who) else {
                    return;
                };
                let places = &self.devices[who];
                self.drive.lose(who, places, &name);
                format!("iCloud has yet to bring back {name} on {who}")
            }
            98 => {
                let who = self.below(TV);
                self.sync_drive(who, true);
                format!("iCloud fills in the folder on {who}")
            }
            99..=100 => {
                let at = self.below(NAMES.len());
                let name = NAMES[at];
                let Some(dropped) = self.drive.regress(name) else {
                    return;
                };
                if !self.held_anywhere(dropped, usize::MAX)
                    || !(self.cloud.holds(dropped)
                        || contents(&self.devices[TV])
                            .values()
                            .any(|at| *at == dropped))
                {
                    self.dropped.insert(dropped);
                }
                format!("a drive conflict keeps the older {name}")
            }
            101 => {
                let who = self.below(TV);
                self.forgot = true;
                self.lose_manifest(who);
                format!("device {who} is restored with no manifest")
            }
            102 => {
                self.forgot = true;
                self.cloud.expired_before.set(self.cloud.log.borrow().len());
                "token expires".to_owned()
            }
            103 => {
                if self.below(4) != 0 {
                    return;
                }
                self.forgot = true;
                let saves: Vec<u64> = self.furthest.values().map(|(_, at)| *at).collect();
                for saved_at in saves {
                    if !self.drive.files.values().any(|c| c.saved_at == saved_at)
                        && !self
                            .devices
                            .iter()
                            .any(|places| contents(places).values().any(|at| *at == saved_at))
                    {
                        self.dropped.insert(saved_at);
                    }
                }
                self.reset = true;
                self.cloud.wipe();
                "cloudkit is reset".to_owned()
            }
            104 => {
                let batch = self.below(3);
                self.cloud.batch.set(batch);
                format!("batches of {batch}")
            }
            105 => {
                self.die(who);
                "the app dies".to_owned()
            }
            106 => {
                self.cloud.fail_save.set(true);
                "next save to the cloud fails".to_owned()
            }
            107 => {
                self.cloud.fail_fetch.set(true);
                "next fetch fails".to_owned()
            }
            108..=110 => {
                self.cloud.delay_save.set(true);
                "next save to the cloud is slow".to_owned()
            }
            111..=114 => {
                self.cloud.land();
                "slow saves land".to_owned()
            }
            115..=116 => {
                let at = self.below(NAMES.len());
                self.cloud.refused.borrow_mut().insert(NAMES[at].to_owned());
                format!("cloudkit refuses {}", NAMES[at])
            }
            117 => {
                self.cloud.refused.borrow_mut().clear();
                "cloudkit takes everything again".to_owned()
            }
            _ => {
                let at = self.below(NAMES.len());
                self.cloud.missed.borrow_mut().insert(NAMES[at].to_owned());
                "next fetch misses a record".to_owned()
            }
        };
        self.log.push(format!("device {who}: {op}"));
    }

    /// Lets iCloud Drive deliver everything, every device run a full pass, and
    /// the player answer whatever they are asked.
    fn round(&mut self) {
        for who in 0..3 {
            if who < TV {
                self.sync_drive(who, true);
            }
            self.run(who, Mode::Full);
            if !self.missing[who].is_empty() {
                let delete = self.below(3) == 0;
                self.answer(who, delete);
            }
        }
    }

    /// Lets every queued thing finish and every device sync until nothing
    /// changes.
    fn settle(&mut self) {
        self.cloud.batch.set(0);
        self.cloud.fail_save.set(false);
        self.cloud.fail_fetch.set(false);
        self.cloud.delay_save.set(false);
        self.cloud.refused.borrow_mut().clear();
        self.cloud.missed.borrow_mut().clear();
        self.cloud.land();
        // A quiet round can still be working: a refetch shows only on the
        // round after.
        let mut last = self.state();
        let mut quiet = 0;
        while quiet < 2 && self.rounds < 16 {
            self.round();
            self.rounds += 1;
            let now = self.state();
            quiet = if now == last { quiet + 1 } else { 0 };
            last = now;
        }
    }

    /// Every device's records and the cloud's.
    fn state(&self) -> Vec<BTreeMap<String, u64>> {
        let mut state: Vec<_> = self.devices.iter().map(contents).collect();
        state.push(self.cloud.files());
        state
    }

    /// What is wrong once things have gone quiet, if anything.
    fn problem(&mut self) -> Option<String> {
        let settled = self.state();
        self.round();
        for (name, (ticks, saved_at)) in &self.furthest {
            let deleted = self.deleted_to.get(name).is_some_and(|to| to > ticks)
                || self.deleted.contains(saved_at)
                || self.dropped.contains(saved_at);
            let somewhere = settled
                .iter()
                .any(|one| one.values().any(|held| held == saved_at));
            if !deleted && !somewhere {
                return Some(format!(
                    "the save of {name} furthest along, tick {ticks}, time {saved_at}, was lost"
                ));
            }
        }
        if self.state() != settled {
            return Some("still moving".to_owned());
        }
        if !self.forgot {
            for (name, saved_at) in &self.deleted_for_good {
                if settled.iter().any(|one| one.get(name) == Some(saved_at)) {
                    return Some(format!("the deleted save {name} {saved_at} came back"));
                }
            }
        }
        if self.missing.iter().any(|names| !names.is_empty()) {
            return Some(format!("still asking: {:?}", self.missing));
        }
        // An autosave or finished match gone from an iCloud Drive folder just
        // stays gone from there; everywhere else, every copy must agree.
        let tv = &settled[TV];
        let agrees = |folder: &BTreeMap<String, u64>| {
            folder.iter().all(|(name, at)| tv.get(name) == Some(at))
                && tv
                    .keys()
                    .all(|name| folder.contains_key(name) || !player_kept(name))
        };
        if (settled[TV + 1] != *tv && !self.reset) || !settled[..TV].iter().all(agrees) {
            return Some(format!("devices differ: {settled:?}"));
        }
        None
    }
}

/// One random history, and what is wrong at its end, if anything.
fn run_history(seed: u64) -> Option<String> {
    let mut world = World::new(seed);
    for _ in 0..70 {
        world.step();
    }
    world.settle();
    let problem = world.problem()?;
    Some(format!("seed {seed}: {problem}\n{}", world.log.join("\n")))
}

#[test]
fn any_history_settles_with_every_device_alike_and_no_save_lost() {
    // Set AFTERMINE_MIRROR_HISTORIES to search more widely.
    let histories: u64 =
        std::env::var("AFTERMINE_MIRROR_HISTORIES").map_or(12, |count| count.parse().unwrap());
    let workers = std::thread::available_parallelism().map_or(1, usize::from) as u64;
    let next = AtomicU64::new(0);
    let problems: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut found = Vec::new();
                    loop {
                        let seed = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if seed >= histories {
                            return found;
                        }
                        found.extend(run_history(seed));
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    let summary: Vec<&str> = problems
        .iter()
        .filter_map(|problem| problem.lines().next())
        .collect();
    assert!(
        problems.is_empty(),
        "{}\n\nfirst history:\n{}",
        summary.join("\n"),
        problems.first().map_or("", String::as_str)
    );
}
