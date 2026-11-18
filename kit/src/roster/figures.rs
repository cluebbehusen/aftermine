//! The figure lines of a Roster page and of the shell's training and build
//! cards: toughness, pace, build time, weapons and income, all read from the
//! stats table.

use super::names::{entity_name, plural_name};
use aftermine_sim::stats::{BuildingKind, UnitKind, WeaponStats};

/// The figures a player weighs before buying a unit, on one line:
/// toughness, pace, build time, sight. Weapons get their own lines. A
/// cradle-built unit has no build time of its own; its cradle's page
/// gives the crew times.
pub fn unit_stat_line(kind: UnitKind) -> String {
    let stats = kind.stats();
    let speed = stats.speed.to_num::<f32>() * aftermine_sim::TICKS_PER_SECOND as f32;
    let assembled = BuildingKind::ALL
        .into_iter()
        .any(|cradle| cradle.assembles() == Some(kind));
    let build = if assembled {
        String::new()
    } else {
        let seconds = stats.train_ticks as f32 / aftermine_sim::TICKS_PER_SECOND as f32;
        format!(" | {seconds:.1} s build")
    };
    format!(
        "{} hp | {speed:.1} tiles/s{build} | sight {}",
        stats.max_hp, stats.vision
    )
}

/// The figures a player weighs before placing a structure, on one line:
/// toughness, build time, footprint, sight. A cradle's time depends on
/// its crew, so [`cradle_crew_line`] gives it instead.
pub fn building_stat_line(kind: BuildingKind) -> String {
    let stats = kind.base_stats();
    let build = if kind.assembles().is_none() {
        format!(
            " | {:.1} s build",
            stats.construction.build_ticks as f32 / aftermine_sim::TICKS_PER_SECOND as f32
        )
    } else {
        String::new()
    };
    let (width, height) = kind.size();
    format!(
        "{} hp{build} | {width}x{height} | sight {}",
        stats.max_hp, stats.vision
    )
}

/// How long a full crew of each kind of worker takes to build a cradle.
pub fn cradle_crew_line(kind: BuildingKind) -> Option<String> {
    kind.assembles()?;
    let ticks = kind.base_stats().construction.build_ticks;
    let crew = aftermine_sim::stats::ASSEMBLY_CREW_CAP;
    let mut workers: Vec<UnitKind> = UnitKind::ALL
        .into_iter()
        .filter(|worker| worker.stats().harvest.is_some() && worker.stats().build_rate > 0)
        .collect();
    workers.sort_by_key(|worker| worker.stats().build_rate);
    let times: Vec<String> = workers
        .iter()
        .map(|worker| {
            let rate = u32::try_from(crew).unwrap_or(u32::MAX) * worker.stats().build_rate;
            let seconds = ticks as f32 / rate as f32 / aftermine_sim::TICKS_PER_SECOND as f32;
            format!(
                "{seconds:.0} s with {crew} {}",
                plural_name(&entity_name(worker.name()))
            )
        })
        .collect();
    Some(format!("Takes {}.", times.join(", or ")))
}

/// Economy details shared by build cards and the codex.
pub fn building_economy_lines(kind: BuildingKind) -> Vec<String> {
    use aftermine_sim::{ExtractorIncome, ExtractorSupport};
    let radius = aftermine_sim::stats::EXTRACTOR_SUPPORT_RADIUS;
    let rate = |tier, support| ExtractorIncome::new(tier, support).scrap_per_minute();
    let remote = rate(0, ExtractorSupport::Remote);
    let supported = rate(0, ExtractorSupport::Supported);
    let per_minute = |period: u64| 60 * u64::from(aftermine_sim::TICKS_PER_SECOND) / period;
    match kind {
        BuildingKind::Reclaimer => vec![format!(
            "Makes {} scrap/min.",
            aftermine_sim::stats::passive_scrap_per_minute(0)
        )],
        BuildingKind::Extractor => vec![format!(
            "Makes {remote} scrap/min, or {supported} scrap/min with your Foundry within {radius} tiles."
        )],
        BuildingKind::Foundry => vec![
            format!(
                "Raises the output of your Extractors within {radius} tiles from {remote} to {supported} scrap/min."
            ),
            format!(
                "After the first {}, each Foundry makes {} scrap/min.",
                duration_words(aftermine_sim::stats::FOUNDRY_DRIP_START_TICK),
                per_minute(aftermine_sim::stats::FOUNDRY_DRIP_PERIOD)
            ),
        ],
        _ => Vec::new(),
    }
}

/// A span of ticks in whole minutes, or seconds when it isn't one.
fn duration_words(ticks: u64) -> String {
    let seconds = ticks / u64::from(aftermine_sim::TICKS_PER_SECOND);
    match (seconds / 60, seconds % 60) {
        (1, 0) => "minute".to_string(),
        (minutes, 0) => format!("{minutes} minutes"),
        _ => format!("{seconds} seconds"),
    }
}

fn weapon_line(weapon: &WeaponStats) -> String {
    let targets = match (
        weapon.targets.covers(aftermine_sim::stats::Domain::Ground),
        weapon.targets.covers(aftermine_sim::stats::Domain::Air),
    ) {
        (true, true) => "ground and air",
        (true, false) => "ground",
        (false, true) => "air",
        (false, false) => "nothing",
    };
    let flavor = if let Some(projectile) = weapon.projectile {
        match projectile.payload {
            aftermine_sim::stats::ProjectileKind::Shell => " | shell",
            aftermine_sim::stats::ProjectileKind::Missile => " | missile",
            aftermine_sim::stats::ProjectileKind::Bomb => " | bomb",
        }
    } else if weapon.indirect {
        " | indirect"
    } else {
        ""
    };
    let splash = if weapon.splash.is_some() {
        " | splash"
    } else {
        ""
    };
    let range = if weapon.minimum_range > chassis::fx::Fx::ZERO {
        format!(
            "{:.1}-{:.1} tiles",
            weapon.minimum_range.to_num::<f32>(),
            weapon.range.to_num::<f32>()
        )
    } else {
        format!("{:.1} tiles", weapon.range.to_num::<f32>())
    };
    let damage = match weapon.building_damage {
        Some(structure) => format!("{} dmg, {structure} vs structures", weapon.damage),
        None => format!("{} dmg", weapon.damage),
    };
    format!("{damage} | {range} | {targets}{flavor}{splash}")
}

/// Human lines for a kind's weapons, from the stats table. Identical
/// guns share one line that counts them.
pub fn weapon_lines(kind: UnitKind) -> Vec<String> {
    counted_weapon_lines(kind.stats().weapons)
}

fn counted_weapon_lines(weapons: &[WeaponStats]) -> Vec<String> {
    let mut lines: Vec<(String, usize)> = Vec::new();
    for line in weapons.iter().map(weapon_line) {
        match lines.last_mut() {
            Some((last, count)) if *last == line => *count += 1,
            _ => lines.push((line, 1)),
        }
    }
    lines
        .into_iter()
        .map(|(line, count)| {
            if count > 1 {
                format!("{count} guns | {line}")
            } else {
                line
            }
        })
        .collect()
}

/// A kind's point defense on one line, for kinds that carry it.
pub fn point_defense_line(kind: UnitKind) -> Option<String> {
    let defense = kind.stats().point_defense?;
    Some(format!(
        "Point defense | {} shots | {:.1} tiles | regains 1 every {}",
        defense.charges,
        defense.radius.to_num::<f32>(),
        tick_time_label(u32::from(defense.recharge_ticks))
    ))
}

/// A structure's weapon lines at `tier`, for the Roster page.
pub fn building_weapon_lines(kind: BuildingKind, tier: u8) -> Vec<String> {
    counted_weapon_lines(kind.tier_stats(tier).weapons)
}

/// A span of ticks as seconds to a tenth, rounded up: "4s", "2.5s".
pub fn tick_time_label(ticks: u32) -> String {
    let per_second = aftermine_sim::TICKS_PER_SECOND;
    let tenths = ticks.saturating_mul(10).div_ceil(per_second);
    let whole = tenths / 10;
    if tenths.is_multiple_of(10) {
        format!("{whole}s")
    } else {
        format!("{whole}.{}s", tenths % 10)
    }
}
