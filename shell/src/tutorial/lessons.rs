//! What each lesson says and what completes each of its steps. Paragraphs
//! wrap to the card; `{token}`s name the player's current bindings.

use super::{Copy, Goal, Sortie, Squad, Stance, Step};
use chassis::grid::TilePos;
use oxide_sim::{BuildingKind, UnitKind};

const ZOOM_WHEEL: &str = "Scroll the mouse wheel to zoom in and out.";
const ZOOM_PINCH: &str = "Pinch with two fingers to zoom in and out.";
const UNIT_PANEL: &str = "The bottom panel shows the selected unit's health and weapons, and the orders you can give it.";
const DOUBLE: &str = "You can also double-click a Sentinel to select every Sentinel on screen.";
const DOUBLE_TAP: &str = "You can also double-tap a Sentinel to select every Sentinel on screen.";
const TOUCH_BOX: &str = "Hold two fingers on the ground until a box appears. Move your fingers \
    to stretch the box around all four Sentinels, then lift one finger.";

pub(super) const COMMAND: [Step; 5] = [
    Step {
        title: "Move the camera",
        copy: Copy {
            mouse_keys: &[
                "Press {pan} or drag with the middle mouse button to move the camera. You can also click the minimap to jump to a spot.",
                ZOOM_WHEEL,
            ],
            mouse: &[
                "Drag with the middle mouse button to move the camera. You can also click the minimap to jump to a spot.",
                ZOOM_WHEEL,
            ],
            touch_keys: &[
                "Drag one finger across the ground, or press {pan}, to move the camera.",
                ZOOM_PINCH,
            ],
            touch: &[
                "Drag one finger across the ground to move the camera.",
                ZOOM_PINCH,
            ],
        },
        goal: Goal::Look(4.0),
    },
    Step {
        title: "Select a Sentinel",
        copy: Copy {
            mouse_keys: &[
                "Your four Sentinels are at the west edge of the map. The minimap shows where. Click one to select it.",
                UNIT_PANEL,
            ],
            mouse: &[
                "Your four Sentinels are at the west edge of the map. The minimap shows where. Click one to select it.",
                UNIT_PANEL,
            ],
            touch_keys: &[
                "Your four Sentinels are at the west edge of the map. The minimap shows where. Tap one to select it.",
                UNIT_PANEL,
            ],
            touch: &[
                "Your four Sentinels are at the west edge of the map. The minimap shows where. Tap one to select it.",
                UNIT_PANEL,
            ],
        },
        goal: Goal::Select(1),
    },
    Step {
        title: "Select the squad",
        copy: Copy {
            mouse_keys: &[
                "Drag a box around all four Sentinels to select them together.",
                DOUBLE,
                "Shift-click a unit to add it to the selection or remove it.",
            ],
            mouse: &[
                "Drag a box around all four Sentinels to select them together.",
                DOUBLE,
            ],
            touch_keys: &[TOUCH_BOX, DOUBLE_TAP],
            touch: &[TOUCH_BOX, DOUBLE_TAP],
        },
        goal: Goal::Select(4),
    },
    Step {
        title: "Find the enemy",
        copy: Copy {
            mouse_keys: &[
                "Right-click open ground to move the squad there. Head east until you find the enemy Fabricator.",
            ],
            mouse: &[
                "Right-click open ground to move the squad there. Head east until you find the enemy Fabricator.",
            ],
            touch_keys: &[
                "Long-press open ground to move the squad there. Keep your finger down until the ring fills. Head east until you find the enemy Fabricator.",
            ],
            touch: &[
                "Long-press open ground to move the squad there. Keep your finger down until the ring fills. Head east until you find the enemy Fabricator.",
            ],
        },
        goal: Goal::Find,
    },
    Step {
        title: "Destroy the Fabricator",
        copy: Copy {
            mouse_keys: &[
                "Right-click the Fabricator to send the whole squad to attack it.",
                "To move and fight anything on the way, press {hunt} and then click the ground. Hunting units stop to chase and attack the enemies they meet.",
            ],
            mouse: &[
                "Right-click the Fabricator to send the whole squad to attack it.",
                "To move and fight anything on the way, click Hunt in the bottom panel and then click the ground. Hunting units stop to chase and attack the enemies they meet.",
            ],
            touch_keys: &[
                "Long-press the Fabricator to send the whole squad to attack it.",
                "To move and fight anything on the way, tap Hunt in the bottom panel or press {hunt}, and then tap the ground. Hunting units stop to chase and attack the enemies they meet.",
            ],
            touch: &[
                "Long-press the Fabricator to send the whole squad to attack it.",
                "To move and fight anything on the way, tap Hunt in the bottom panel and then tap the ground. Hunting units stop to chase and attack the enemies they meet.",
            ],
        },
        goal: Goal::Destroy,
    },
];

pub(super) const COMMAND_OUTRO: &str =
    "You can now move the camera, select units, move them, and attack.";

const HAUL: &str = "The Harvester collects scrap at the pile and brings it back to your \
    Foundry. The SCRAP counter at the top left shows how much you have.";
const MORE_WORKERS: &str =
    "More Harvesters bring in scrap faster. Harvesters also construct buildings.";
const IDLE: &str = "Your new Harvester is idle. The idle badge in the top bar shows how many \
    Harvesters have nothing to do.";
const NEAR_PAYS: &str = "An Extractor earns scrap without any hauling. Within 8 tiles of a \
    Foundry, it earns 180 scrap a minute.";
const FAR_FRAME: &[&str] = &[
    "Your Surveyor has found a second frame to the east. Restore it the same way.",
    "Farther than 8 tiles from a Foundry, an Extractor earns 120 scrap a minute. The top bar shows your total passive income, including a small trickle from your Foundry.",
];

pub(super) const SCRAP: [Step; 5] = [
    Step {
        title: "Collect scrap",
        copy: Copy {
            mouse_keys: &["Select a Harvester, then right-click a scrap pile.", HAUL],
            mouse: &["Select a Harvester, then right-click a scrap pile.", HAUL],
            touch_keys: &["Select a Harvester, then long-press a scrap pile.", HAUL],
            touch: &["Select a Harvester, then long-press a scrap pile.", HAUL],
        },
        goal: Goal::Deposit,
    },
    Step {
        title: "Train a Harvester",
        copy: Copy {
            mouse_keys: &[
                "Click your Foundry, then click Harvester in the bottom panel or press {train}.",
                MORE_WORKERS,
            ],
            mouse: &[
                "Click your Foundry, then click Harvester in the bottom panel.",
                MORE_WORKERS,
            ],
            touch_keys: &[
                "Tap your Foundry, then tap Harvester in the bottom panel or press {train}.",
                MORE_WORKERS,
            ],
            touch: &[
                "Tap your Foundry, then tap Harvester in the bottom panel.",
                MORE_WORKERS,
            ],
        },
        goal: Goal::Train(UnitKind::Harvester),
    },
    Step {
        title: "Put every Harvester to work",
        copy: Copy {
            mouse_keys: &[
                IDLE,
                "Click the badge or press {idle} to select an idle Harvester, then right-click a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
            mouse: &[
                IDLE,
                "Click the badge to select an idle Harvester, then right-click a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
            touch_keys: &[
                IDLE,
                "Tap the badge or press {idle} to select an idle Harvester, then long-press a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
            touch: &[
                IDLE,
                "Tap the badge to select an idle Harvester, then long-press a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
        },
        goal: Goal::Harvest(3),
    },
    Step {
        title: "Restore an Extractor",
        copy: Copy {
            mouse_keys: &[
                "The 2x2 frame near your Foundry is a derelict Extractor. To restore it, select a Harvester, click Build or press {build}, choose Extractor in the Economy column, and click the frame.",
                NEAR_PAYS,
            ],
            mouse: &[
                "The 2x2 frame near your Foundry is a derelict Extractor. To restore it, select a Harvester, click Build, choose Extractor in the Economy column, and click the frame.",
                NEAR_PAYS,
            ],
            touch_keys: &[
                "The 2x2 frame near your Foundry is a derelict Extractor. To restore it, select a Harvester, tap Build or press {build}, and choose Extractor in the Economy column. Drag the building's outline onto the frame and tap the check mark.",
                NEAR_PAYS,
            ],
            touch: &[
                "The 2x2 frame near your Foundry is a derelict Extractor. To restore it, select a Harvester, tap Build, and choose Extractor in the Economy column. Drag the building's outline onto the frame and tap the check mark.",
                NEAR_PAYS,
            ],
        },
        goal: Goal::Build(BuildingKind::Extractor, 1),
    },
    Step {
        title: "Restore the far frame",
        copy: Copy {
            mouse_keys: FAR_FRAME,
            mouse: FAR_FRAME,
            touch_keys: FAR_FRAME,
            touch: FAR_FRAME,
        },
        goal: Goal::Build(BuildingKind::Extractor, 2),
    },
];

pub(super) const SCRAP_OUTRO: &str = "You can now collect scrap, train Harvesters, and restore \
    Extractors. In a match, keep every Harvester busy and restore Extractors early to grow your \
    income.";

const MORE_BUILDERS: &str = "More Harvesters on one site build it faster. To add one, select it \
    and right-click the site.";
const MORE_BUILDERS_TOUCH: &str = "More Harvesters on one site build it faster. To add one, \
    select it and long-press the site.";
const PAID_IN_ORDER: &str = "Each site is paid for when you place it, and your Harvester builds \
    them one after another.";
const RALLY_USE: &str = "Armed machines hunt their way to the rally point. A Harvester rallied \
    to a scrap pile starts collecting there.";
const FABRICATOR_UNLOCKS: &str = "The Fabricator trains machines the Foundry can't, such as \
    Lancers and Flakhounds. It also unlocks more buildings, such as the Airworks and the \
    Crucible.";

pub(super) const BUILD: [Step; 5] = [
    Step {
        title: "Build a Fabricator",
        copy: Copy {
            mouse_keys: &[
                "Select a Harvester, then click Build or press {build}. Buildings are sorted into columns such as Economy, Production, and Defense. Point at a card to see what it does, or what it still needs.",
                "Choose Fabricator in the Production column and click open ground you have explored. The bar above the bottom panel says whether the spot can be built on.",
                MORE_BUILDERS,
            ],
            mouse: &[
                "Select a Harvester, then click Build. Buildings are sorted into columns such as Economy, Production, and Defense. Point at a card to see what it does, or what it still needs.",
                "Choose Fabricator in the Production column and click open ground you have explored. The bar above the bottom panel says whether the spot can be built on.",
                MORE_BUILDERS,
            ],
            touch_keys: &[
                "Select a Harvester, then tap Build or press {build}. Buildings are sorted into columns such as Economy, Production, and Defense.",
                "Choose Fabricator in the Production column. Drag the building's outline onto open ground you have explored and tap the check mark. The bar above the bottom panel says whether the spot can be built on.",
                MORE_BUILDERS_TOUCH,
            ],
            touch: &[
                "Select a Harvester, then tap Build. Buildings are sorted into columns such as Economy, Production, and Defense.",
                "Choose Fabricator in the Production column. Drag the building's outline onto open ground you have explored and tap the check mark. The bar above the bottom panel says whether the spot can be built on.",
                MORE_BUILDERS_TOUCH,
            ],
        },
        goal: Goal::Build(BuildingKind::Fabricator, 1),
    },
    Step {
        title: "Build three Barricades",
        copy: Copy {
            mouse_keys: &[
                "Barricades are cheap walls. Choose Barricade in the Defense column, then hold Shift and click three spots. You can also click and drag to lay a row.",
                PAID_IN_ORDER,
            ],
            mouse: &[
                "Barricades are cheap walls. Choose Barricade in the Defense column, then click and drag across open ground to lay a row of three.",
                PAID_IN_ORDER,
            ],
            touch_keys: &[
                "Barricades are cheap walls. Choose Barricade in the Defense column. Drag the handle on the outline's edge to stretch it into a row of three, then tap the check mark.",
                PAID_IN_ORDER,
            ],
            touch: &[
                "Barricades are cheap walls. Choose Barricade in the Defense column. Drag the handle on the outline's edge to stretch it into a row of three, then tap the check mark.",
                PAID_IN_ORDER,
            ],
        },
        goal: Goal::Build(BuildingKind::Barricade, 3),
    },
    Step {
        title: "Set a rally point",
        copy: Copy {
            mouse_keys: &[
                "A rally point is where new machines go when they are trained. Select your Fabricator and right-click the ground where they should gather, or press {rally} and click the spot.",
                RALLY_USE,
            ],
            mouse: &[
                "A rally point is where new machines go when they are trained. Select your Fabricator and right-click the ground where they should gather, or click Set next to the flag in the bottom panel and click the spot.",
                RALLY_USE,
            ],
            touch_keys: &[
                "A rally point is where new machines go when they are trained. Select your Fabricator and long-press the ground where they should gather.",
                RALLY_USE,
            ],
            touch: &[
                "A rally point is where new machines go when they are trained. Select your Fabricator and long-press the ground where they should gather.",
                RALLY_USE,
            ],
        },
        goal: Goal::Rally,
    },
    Step {
        title: "Train a Lancer",
        copy: Copy {
            mouse_keys: &[
                "With the Fabricator selected, click Lancer in the bottom panel or press {train}.",
                FABRICATOR_UNLOCKS,
            ],
            mouse: &[
                "With the Fabricator selected, click Lancer in the bottom panel.",
                FABRICATOR_UNLOCKS,
            ],
            touch_keys: &[
                "With the Fabricator selected, tap Lancer in the bottom panel or press {train}.",
                FABRICATOR_UNLOCKS,
            ],
            touch: &[
                "With the Fabricator selected, tap Lancer in the bottom panel.",
                FABRICATOR_UNLOCKS,
            ],
        },
        goal: Goal::Train(UnitKind::Lancer),
    },
    Step {
        title: "Make a control group",
        copy: Copy {
            mouse_keys: &[
                "A control group saves a selection so you can select it again with one key. Select your Lancer and press {assign}. This saves it as group 1.",
                "Press {recall} to select group 1 again. Press it twice to move the camera to it. You can also click the plus above the minimap to save a group, and click the group there to select it.",
            ],
            mouse: &[
                "A control group saves a selection so you can select it again with one click. Select your Lancer, then click the plus above the minimap to save it as a group.",
                "Click the group above the minimap to select it again.",
            ],
            touch_keys: &[
                "A control group saves a selection so you can select it again with one tap. Select your Lancer, then tap the plus above the minimap or press {assign} to save it as a group.",
                "Tap the group above the minimap or press {recall} to select it again.",
            ],
            touch: &[
                "A control group saves a selection so you can select it again with one tap. Select your Lancer, then tap the plus above the minimap to save it as a group.",
                "Tap the group above the minimap to select it again.",
            ],
        },
        goal: Goal::Group,
    },
];

pub(super) const BUILD_OUTRO: &str = "You can now build, place several buildings at once, set \
    rally points, and use control groups.";

const RAID: &str = "Five enemy Scuttlers are coming for your Harvesters. Scuttlers are fast \
    and cheap and kill Harvesters quickly, but a Sentinel beats a Scuttler one on one.";
const HUNTING: &str = "Hunting machines stop to chase and fight every enemy they meet.";
const ADVANCE_CLICK: &str = "A right-click on the ground is an Advance instead. Advancing \
    machines fire at enemies in range but never stop or chase, so a raid can slip past them.";
const ADVANCE_PRESS: &str = "A long-press on the ground is an Advance instead. Advancing \
    machines fire at enemies in range but never stop or chase, so a raid can slip past them.";
const ROCK: &str = "Two enemy Lancers stand behind the rock line in the middle. Rock blocks \
    shots between ground machines but not sight, so up close you can see them but can't hit \
    them through it.";
const AROUND: &str = "A Lancer can hit targets 5.5 tiles away, more than twice as far as a \
    Sentinel, and kills a Sentinel with one shot. Send your fast Scuttlers around the end of the \
    rock to reach them, with the rest of your army behind.";
const LEVELS: &str = "Three enemy Sentinels hold the high ground in the northeast. Ground \
    machines can only shoot machines on their own level, so neither side can shoot across \
    the cliff.";
const RAMP: &str = "A ramp at the west end of the cliff joins both levels, and machines on it \
    can shoot up and down. Send your army up the ramp to fight.";
const WARDEN: &str = "An enemy Warden is heading for your base. It has 340 HP, more than \
    five Sentinels together.";
const BESIDE_TURRETS: &str = "Running machines move without stopping to fight, so nothing \
    holds them up on the way. Then fight the Warden beside your Turrets.";
const OUTPOST: &str = "The enemy outpost in the east has a Fabricator and a Turret. Destroy both.";

pub(super) const FIGHT: [Step; 5] = [
    Step {
        title: "Stop the raid",
        copy: Copy {
            mouse_keys: &[
                RAID,
                "Select your army, press {hunt}, and click near your Harvesters.",
                HUNTING,
                ADVANCE_CLICK,
            ],
            mouse: &[
                RAID,
                "Select your army, click Hunt in the bottom panel, and click near your Harvesters.",
                HUNTING,
                ADVANCE_CLICK,
            ],
            touch_keys: &[
                RAID,
                "Select your army, tap Hunt in the bottom panel or press {hunt}, and tap near your Harvesters.",
                HUNTING,
                ADVANCE_PRESS,
            ],
            touch: &[
                RAID,
                "Select your army, tap Hunt in the bottom panel, and tap near your Harvesters.",
                HUNTING,
                ADVANCE_PRESS,
            ],
        },
        goal: Goal::Defeat(0),
    },
    Step {
        title: "Get around the rocks",
        copy: Copy {
            mouse_keys: &[ROCK, AROUND],
            mouse: &[ROCK, AROUND],
            touch_keys: &[ROCK, AROUND],
            touch: &[ROCK, AROUND],
        },
        goal: Goal::Defeat(1),
    },
    Step {
        title: "Take the ramp",
        copy: Copy {
            mouse_keys: &[LEVELS, RAMP],
            mouse: &[LEVELS, RAMP],
            touch_keys: &[LEVELS, RAMP],
            touch: &[LEVELS, RAMP],
        },
        goal: Goal::Defeat(2),
    },
    Step {
        title: "Fall back",
        copy: Copy {
            mouse_keys: &[
                WARDEN,
                "Select your army, press {run}, and click near your Turrets.",
                BESIDE_TURRETS,
            ],
            mouse: &[
                WARDEN,
                "Select your army, click Run in the bottom panel, and click near your Turrets.",
                BESIDE_TURRETS,
            ],
            touch_keys: &[
                WARDEN,
                "Select your army, tap Run in the bottom panel or press {run}, and tap near your Turrets.",
                BESIDE_TURRETS,
            ],
            touch: &[
                WARDEN,
                "Select your army, tap Run in the bottom panel, and tap near your Turrets.",
                BESIDE_TURRETS,
            ],
        },
        goal: Goal::Defeat(3),
    },
    Step {
        title: "Destroy the outpost",
        copy: Copy {
            mouse_keys: &[
                OUTPOST,
                "Press {hunt} and click near the outpost to clear the way, then right-click each building to attack it.",
            ],
            mouse: &[
                OUTPOST,
                "Click Hunt and click near the outpost to clear the way, then right-click each building to attack it.",
            ],
            touch_keys: &[
                OUTPOST,
                "Tap Hunt or press {hunt} and tap near the outpost to clear the way, then long-press each building to attack it.",
            ],
            touch: &[
                OUTPOST,
                "Tap Hunt and tap near the outpost to clear the way, then long-press each building to attack it.",
            ],
        },
        goal: Goal::Destroy,
    },
];

/// The scene's enemy squads, which the steps above name by index: the
/// raid, the Lancers behind the rock, the Sentinels on the high ground,
/// and the Warden.
pub(super) const FIGHT_SQUADS: [Squad; 4] = [
    Squad {
        units: &[15, 16, 17, 18, 19],
        sortie: Some(Sortie {
            step: 0,
            toward: TilePos::new(5, 26),
            stance: Stance::Hunt,
        }),
    },
    Squad {
        units: &[20, 21],
        sortie: None,
    },
    Squad {
        units: &[22, 23, 24],
        sortie: None,
    },
    Squad {
        units: &[25],
        sortie: Some(Sortie {
            step: 3,
            toward: TilePos::new(9, 21),
            stance: Stance::Hunt,
        }),
    },
];

pub(super) const FIGHT_OUTRO: &str = "You can now choose between Hunt, Advance, and Run, use \
    each machine for what it does best, and fight around rock and high ground.";

const FOG: &str = "Ground you have never seen is dark. Ground you have seen but can't see \
    right now is dimmed. It shows enemy buildings as you last saw them, but not enemy machines.";
const SURVEYOR: &str = "The Surveyor is your fastest ground machine and sees 9 tiles, but it \
    has no weapons. An enemy base is somewhere to the east.";
const SCOUT_SAFE: &str = "A scouting Surveyor stays out of reach of enemies it has seen.";
const SURVEYOR_RADAR: &str = "A Surveyor that stands still for 2 seconds also unfolds a \
    short-range radar. Enemies up to 12 tiles away that it can't see show up as blips.";
const ARRAY: &str = "An Array is a building with a long-range radar. It sees 9 tiles around \
    itself and shows enemies up to 20 tiles away as blips.";
const BLIPS: &str = "A blip shows where an enemy is, but not what it is. A full diamond means it \
    is on the ground, and only the top half of a diamond means it is in the air.";
const AIRCRAFT: &str = "Two Buzzards wait at the enemy base. Buzzards are aircraft. They fly \
    over rock and attack only targets on the ground.";
const ANTI_AIR: &str = "Most ground machines can't shoot aircraft. Flak Turrets and Flakhounds \
    shoot only aircraft. Sentinels can shoot both, but are weak against aircraft.";
const INBOUND: &str = "The Buzzards are flying to your base. Watch for their blips on your \
    Array's radar before they come into sight.";
const FLAK_FIRST: &str = "Your Flak Turret hits them from 5.5 tiles away. A Buzzard must come \
    within 3 tiles to fire, so the turret shoots first.";
const IN_HARMS_WAY: &str = "Buzzards attack the nearest target, so Harvesters left idle beside \
    your new buildings are in danger.";
const WELDING: &str = "Harvesters weld damaged buildings and ground machines back to full \
    health. Welding costs scrap for each point of health it restores.";
const IDLE_WELD: &str = "Idle Harvesters also weld damage near them on their own.";
const REPAIR_BAY: &str = "A Repair Bay repairs your machines within 4 tiles of it, up to four \
    at a time. It works quickly once they are out of combat. It also repairs nearby buildings \
    slowly, and pays for all repairs with scrap.";
const WRECKS: &str =
    "A destroyed machine leaves a wreck that holds some of the scrap the machine cost.";
const WRECKS_FADE: &str = "Wrecks deteriorate over time, so collect them soon.";

pub(super) const SUSTAIN: [Step; 7] = [
    Step {
        title: "Scout the east",
        copy: Copy {
            mouse_keys: &[
                FOG,
                SURVEYOR,
                "Select the Surveyor and right-click far to the east to scout there.",
                SCOUT_SAFE,
                SURVEYOR_RADAR,
            ],
            mouse: &[
                FOG,
                SURVEYOR,
                "Select the Surveyor and right-click far to the east to scout there.",
                SCOUT_SAFE,
                SURVEYOR_RADAR,
            ],
            touch_keys: &[
                FOG,
                SURVEYOR,
                "Select the Surveyor and long-press far to the east to scout there.",
                SCOUT_SAFE,
                SURVEYOR_RADAR,
            ],
            touch: &[
                FOG,
                SURVEYOR,
                "Select the Surveyor and long-press far to the east to scout there.",
                SCOUT_SAFE,
                SURVEYOR_RADAR,
            ],
        },
        goal: Goal::Find,
    },
    Step {
        title: "Build an Array",
        copy: Copy {
            mouse_keys: &[
                ARRAY,
                BLIPS,
                "Select a Harvester, click Build or press {build}, and choose Array in the Utility column. Click open ground east of your Foundry to place it.",
            ],
            mouse: &[
                ARRAY,
                BLIPS,
                "Select a Harvester, click Build, and choose Array in the Utility column. Click open ground east of your Foundry to place it.",
            ],
            touch_keys: &[
                ARRAY,
                BLIPS,
                "Select a Harvester, tap Build or press {build}, and choose Array in the Utility column. Drag its outline east of your Foundry and tap the check mark.",
            ],
            touch: &[
                ARRAY,
                BLIPS,
                "Select a Harvester, tap Build, and choose Array in the Utility column. Drag its outline east of your Foundry and tap the check mark.",
            ],
        },
        goal: Goal::Build(BuildingKind::Array, 1),
    },
    Step {
        title: "Build a Flak Turret",
        copy: Copy {
            mouse_keys: &[
                AIRCRAFT,
                ANTI_AIR,
                "Select a Harvester, click Build or press {build}, and choose Flak Turret in the Defense column. Click open ground east of your Foundry to place it.",
            ],
            mouse: &[
                AIRCRAFT,
                ANTI_AIR,
                "Select a Harvester, click Build, and choose Flak Turret in the Defense column. Click open ground east of your Foundry to place it.",
            ],
            touch_keys: &[
                AIRCRAFT,
                ANTI_AIR,
                "Select a Harvester, tap Build or press {build}, and choose Flak Turret in the Defense column. Drag its outline east of your Foundry and tap the check mark.",
            ],
            touch: &[
                AIRCRAFT,
                ANTI_AIR,
                "Select a Harvester, tap Build, and choose Flak Turret in the Defense column. Drag its outline east of your Foundry and tap the check mark.",
            ],
        },
        goal: Goal::Build(BuildingKind::FlakTurret, 1),
    },
    Step {
        title: "Shoot down the Buzzards",
        copy: Copy {
            mouse_keys: &[
                INBOUND,
                FLAK_FIRST,
                IN_HARMS_WAY,
                "Click the badge or press {idle} to select an idle Harvester, then right-click a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
            mouse: &[
                INBOUND,
                FLAK_FIRST,
                IN_HARMS_WAY,
                "Click the badge to select an idle Harvester, then right-click a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
            touch_keys: &[
                INBOUND,
                FLAK_FIRST,
                IN_HARMS_WAY,
                "Tap the badge or press {idle} to select an idle Harvester, then long-press a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
            touch: &[
                INBOUND,
                FLAK_FIRST,
                IN_HARMS_WAY,
                "Tap the badge to select an idle Harvester, then long-press a scrap pile. Repeat until all three Harvesters are collecting.",
            ],
        },
        goal: Goal::Defeat(0),
    },
    Step {
        title: "Weld the damage",
        copy: Copy {
            mouse_keys: &[
                WELDING,
                "Select a Harvester and right-click a damaged building or machine to weld it.",
                IDLE_WELD,
            ],
            mouse: &[
                WELDING,
                "Select a Harvester and right-click a damaged building or machine to weld it.",
                IDLE_WELD,
            ],
            touch_keys: &[
                WELDING,
                "Select a Harvester and long-press a damaged building or machine to weld it.",
                IDLE_WELD,
            ],
            touch: &[
                WELDING,
                "Select a Harvester and long-press a damaged building or machine to weld it.",
                IDLE_WELD,
            ],
        },
        goal: Goal::Weld,
    },
    Step {
        title: "Build a Repair Bay",
        copy: Copy {
            mouse_keys: &[
                REPAIR_BAY,
                "Select a Harvester, click Build or press {build}, and choose Repair Bay in the Utility column. Click open ground next to your Foundry to place it.",
            ],
            mouse: &[
                REPAIR_BAY,
                "Select a Harvester, click Build, and choose Repair Bay in the Utility column. Click open ground next to your Foundry to place it.",
            ],
            touch_keys: &[
                REPAIR_BAY,
                "Select a Harvester, tap Build or press {build}, and choose Repair Bay in the Utility column. Drag its outline next to your Foundry and tap the check mark.",
            ],
            touch: &[
                REPAIR_BAY,
                "Select a Harvester, tap Build, and choose Repair Bay in the Utility column. Drag its outline next to your Foundry and tap the check mark.",
            ],
        },
        goal: Goal::Build(BuildingKind::RepairBay, 1),
    },
    Step {
        title: "Collect the wrecks",
        copy: Copy {
            mouse_keys: &[
                WRECKS,
                "Select a Harvester and right-click a wreck to collect it, the same way as a scrap pile.",
                WRECKS_FADE,
            ],
            mouse: &[
                WRECKS,
                "Select a Harvester and right-click a wreck to collect it, the same way as a scrap pile.",
                WRECKS_FADE,
            ],
            touch_keys: &[
                WRECKS,
                "Select a Harvester and long-press a wreck to collect it, the same way as a scrap pile.",
                WRECKS_FADE,
            ],
            touch: &[
                WRECKS,
                "Select a Harvester and long-press a wreck to collect it, the same way as a scrap pile.",
                WRECKS_FADE,
            ],
        },
        goal: Goal::CollectWreck,
    },
];

/// The two Buzzards, which set out for the base once the Flak Turret
/// stands. They run past a Surveyor left watching theirs and start to
/// fight just short of the base.
pub(super) const SUSTAIN_SQUADS: [Squad; 1] = [Squad {
    units: &[4, 5],
    sortie: Some(Sortie {
        step: 3,
        toward: TilePos::new(9, 12),
        stance: Stance::RunTo(TilePos::new(17, 12)),
    }),
}];

pub(super) const SUSTAIN_OUTRO: &str = "You can now scout, use radar, defend against aircraft, \
    repair, and collect wrecks. That completes Basic Training.";
