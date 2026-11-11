//! What each lesson says and what completes each of its steps. Paragraphs
//! wrap to the card and are worded as [`crate::wording`] describes;
//! `{token}`s name the player's current bindings.

use super::{Goal, Sortie, Squad, Stance, Step};
use crate::wording::Para;
use aftermine_sim::{BuildingKind, UnitKind};
use chassis::grid::TilePos;

const ZOOM_WHEEL: &str = "Scroll the mouse wheel to zoom in and out.";
const ZOOM_PINCH: &str = "Pinch with two fingers to zoom in and out.";
const ZOOM_STICK: &str = "Push the right stick up or down to zoom in and out.";
const UNIT_PANEL: &str = "The bottom panel shows the selected unit's health and weapons, and the orders you can give it.";
const UNIT_PANEL_PAD: &str = "The bottom panel shows the selected unit's health and weapons. Press {Y} to see the orders you can give it.";
const TOUCH_BOX: &str = "Hold two fingers on the ground until a box appears. Move your fingers \
    to stretch the box around all four Sentinels, then lift one finger.";

pub(super) const COMMAND_BRIEFING: &str = "Your four Sentinels are at the west edge of the map. \
    An enemy Fabricator is somewhere to the east. Move the camera, select your squad, and send it \
    east to find the Fabricator. Then destroy it.";

pub(super) const COMMAND: [Step; 5] = [
    Step {
        title: "Move the camera",
        copy: &[
            Para::Each {
                mouse: "[Press {pan} or drag|Drag] with the middle mouse button to move the camera. You can also click the minimap to jump to a spot.",
                touch: "Drag one finger across the ground[, or press {pan},] to move the camera.",
                pad: "Push the left stick to move the camera. The reticle in the middle of the screen is your pointer.",
            },
            Para::Each {
                mouse: ZOOM_WHEEL,
                touch: ZOOM_PINCH,
                pad: ZOOM_STICK,
            },
        ],
        goal: Goal::Look(4.0),
    },
    Step {
        title: "Select a Sentinel",
        copy: &[
            Para::Pointer {
                pointer: "Your four Sentinels are at the west edge of the map. The minimap shows where. {Click} one to select it.",
                pad: "Your four Sentinels are at the west edge of the map. The minimap shows where. Move the reticle onto one and press {A} to select it.",
            },
            Para::Pointer {
                pointer: UNIT_PANEL,
                pad: UNIT_PANEL_PAD,
            },
        ],
        goal: Goal::Select(1),
    },
    Step {
        title: "Select the squad",
        copy: &[
            Para::Each {
                mouse: "Drag a box around all four Sentinels to select them together.",
                touch: TOUCH_BOX,
                pad: "Hold {A} and move the reticle across all four Sentinels, then let go to select them together.",
            },
            Para::Pointer {
                pointer: "You can also double-{click} a Sentinel to select every Sentinel on screen.",
                pad: "You can also press {A} twice on a Sentinel to select every Sentinel on screen.",
            },
            Para::Each {
                mouse: "[Shift-click a unit to add it to the selection or remove it.]",
                touch: "",
                pad: "Hold {LT} and press {A} on a unit to add it to the selection or remove it.",
            },
        ],
        goal: Goal::Select(4),
    },
    Step {
        title: "Find the enemy",
        copy: &[Para::Each {
            mouse: "Right-click open ground to move the squad there. Head east until you find the enemy Fabricator.",
            touch: "Long-press open ground to move the squad there. Keep your finger down until the ring fills. Head east until you find the enemy Fabricator.",
            pad: "Move the reticle onto open ground and press {X} to move the squad there. Head east until you find the enemy Fabricator.",
        }],
        goal: Goal::Find,
    },
    Step {
        title: "Destroy the Fabricator",
        copy: &[
            Para::Same("{Right-click} the Fabricator to send the whole squad to attack it."),
            Para::Each {
                mouse: "To move and fight anything on the way, [press {hunt}|click Hunt in the bottom panel] and then click the ground. Hunting units stop to chase and attack the enemies they meet.",
                touch: "To move and fight anything on the way, tap Hunt in the bottom panel[ or press {hunt},] and then tap the ground. Hunting units stop to chase and attack the enemies they meet.",
                pad: "To move and fight anything on the way, press {Y} and point the left stick at Hunt. Press {A} to pick it, then press {A} again on the ground. Hunting units stop to chase and attack the enemies they meet.",
            },
        ],
        goal: Goal::Destroy,
    },
];

pub(super) const COMMAND_OUTRO: &str = "";

const HAUL: &str = "The Harvester collects scrap at the pile and brings it back to your \
    Foundry. The SCRAP counter at the top left shows how much you have.";
const MORE_WORKERS: &str =
    "More Harvesters bring in scrap faster. Harvesters also build structures.";
const IDLE: &str = "Your new Harvester is idle. The idle badge in the top bar shows how many \
    Harvesters have nothing to do.";
const NEAR_PAYS: &str = "An Extractor earns scrap without any hauling. Within 8 tiles of a \
    Foundry, it earns 180 scrap a minute.";
const FAR_FRAME: &[Para] = &[
    Para::Same("Your Surveyor has found a second frame to the east. Restore it the same way."),
    Para::Same(
        "Farther than 8 tiles from a Foundry, an Extractor earns 120 scrap a minute. The top bar shows your total passive income, including a small trickle from your Foundry.",
    ),
];
const IDLE_PICK: Para = Para::Pointer {
    pointer: "{Click} the badge[ or press {idle}] to select an idle Harvester, then {right-click} a scrap pile. Repeat until all three Harvesters are collecting.",
    pad: "Press D-pad down to select an idle Harvester, then press {X} on a scrap pile. Repeat until all three Harvesters are collecting.",
};

pub(super) const SCRAP_BRIEFING: &str = "You start with a Foundry, two Harvesters, and a \
    Surveyor. Everything you build and train costs scrap, so send a Harvester to collect some. \
    Train a third Harvester and keep all three busy, then restore two derelict Extractors, which \
    earn scrap without any hauling.";

pub(super) const SCRAP: [Step; 5] = [
    Step {
        title: "Collect scrap",
        copy: &[
            Para::Same("Select a Harvester, then {right-click} a scrap pile."),
            Para::Same(HAUL),
        ],
        goal: Goal::Deposit,
    },
    Step {
        title: "Train a Harvester",
        copy: &[
            Para::Pointer {
                pointer: "{Click} your Foundry, then {click} Harvester in the bottom panel[ or press {train}].",
                pad: "Select your Foundry and press {Y}. Point the left stick at Harvester and press {A}. The ring stays open so you can train more. Press {B} to close it.",
            },
            Para::Same(MORE_WORKERS),
        ],
        goal: Goal::Train(UnitKind::Harvester),
    },
    Step {
        title: "Put every Harvester to work",
        copy: &[Para::Same(IDLE), IDLE_PICK],
        goal: Goal::Harvest(3),
    },
    Step {
        title: "Restore an Extractor",
        copy: &[
            Para::Each {
                mouse: "The 2x2 frame near your Foundry is a derelict Extractor. To restore it, select a Harvester, click Build[ or press {build}], choose Extractor in the Economy column, and click the frame.",
                touch: "The 2x2 frame near your Foundry is a derelict Extractor. To restore it, select a Harvester, tap Build[ or press {build}], and choose Extractor in the Economy column. Drag the structure's outline onto the frame and tap the check mark.",
                pad: "The 2x2 frame near your Foundry is a derelict Extractor. To restore it, select a Harvester, press {Y}, and pick Build, then Economy, then Extractor. Move the reticle onto the frame and press {A}.",
            },
            Para::Same(NEAR_PAYS),
        ],
        goal: Goal::Build(BuildingKind::Extractor, 1),
    },
    Step {
        title: "Restore the far frame",
        copy: FAR_FRAME,
        goal: Goal::Build(BuildingKind::Extractor, 2),
    },
];

pub(super) const SCRAP_OUTRO: &str =
    "In a match, keep every Harvester busy and restore Extractors early.";

const MORE_BUILDERS: &str = "More Harvesters on one site build it faster. To add one, select it \
    and {right-click} the site.";
const PAID_IN_ORDER: &str = "Each site is paid for when you place it, and your Harvester builds \
    them one after another.";
const RALLY_USE: &str = "Armed units hunt their way to the rally point. A Harvester rallied \
    to a scrap pile starts collecting there.";
const FABRICATOR_UNLOCKS: &str = "The Fabricator trains units the Foundry can't, such as \
    Lancers and Flakhounds. It also unlocks more structures, such as the Airworks and the \
    Crucible.";

pub(super) const BUILD_BRIEFING: &str = "You have three Harvesters and scrap to spend. Build a \
    Fabricator to train units the Foundry can't, then a row of Barricades. Set a rally point for \
    the Fabricator, train a Lancer, and save it as a control group so you can select it again \
    quickly.";

pub(super) const BUILD: [Step; 5] = [
    Step {
        title: "Build a Fabricator",
        copy: &[
            Para::Each {
                mouse: "Select a Harvester, then click Build[ or press {build}]. Structures are sorted into columns such as Economy, Production, and Defense. Point at a card to see what it does, or what it still needs.",
                touch: "Select a Harvester, then tap Build[ or press {build}]. Structures are sorted into columns such as Economy, Production, and Defense.",
                pad: "Select a Harvester, press {Y}, and pick Build. Structures are sorted into Economy, Production, Defense and more. Point the left stick at one to see what it does, or what it still needs.",
            },
            Para::Each {
                mouse: "Choose Fabricator in the Production column and click open ground you have explored. The bar above the bottom panel says whether the spot can be built on.",
                touch: "Choose Fabricator in the Production column. Drag the structure's outline onto open ground you have explored and tap the check mark. The bar above the bottom panel says whether the spot can be built on.",
                pad: "Pick Fabricator in Production, move the reticle onto open ground you have explored, and press {A}. The bar above the bottom panel says whether the spot can be built on.",
            },
            Para::Same(MORE_BUILDERS),
        ],
        goal: Goal::Build(BuildingKind::Fabricator, 1),
    },
    Step {
        title: "Build three Barricades",
        copy: &[
            Para::Each {
                mouse: "Barricades are cheap walls. Choose Barricade in the Defense column, then [hold Shift and click three spots. You can also click and drag to lay a row.|click and drag across open ground to lay a row of three.]",
                touch: "Barricades are cheap walls. Choose Barricade in the Defense column. Drag the handle on the outline's edge to stretch it into a row of three, then tap the check mark.",
                pad: "Barricades are cheap walls. Pick Barricade in Defense, then hold {A} and move the reticle to lay a row of three. You can also hold {LT} and press {A} on three spots.",
            },
            Para::Same(PAID_IN_ORDER),
        ],
        goal: Goal::Build(BuildingKind::Barricade, 3),
    },
    Step {
        title: "Set a rally point",
        copy: &[
            Para::Each {
                mouse: "A rally point is where new units go when they are trained. Select your Fabricator and right-click the ground where they should gather, or [press {rally}|click Set next to the flag in the bottom panel] and click the spot.",
                touch: "A rally point is where new units go when they are trained. Select your Fabricator and long-press the ground where they should gather.",
                pad: "A rally point is where new units go when they are trained. Select your Fabricator and press {X} on the ground where they should gather.",
            },
            Para::Same(RALLY_USE),
        ],
        goal: Goal::Rally,
    },
    Step {
        title: "Train a Lancer",
        copy: &[
            Para::Pointer {
                pointer: "With the Fabricator selected, {click} Lancer in the bottom panel[ or press {train}].",
                pad: "With the Fabricator selected, press {Y} and pick Lancer, then press {B} to close the ring.",
            },
            Para::Same(FABRICATOR_UNLOCKS),
        ],
        goal: Goal::Train(UnitKind::Lancer),
    },
    Step {
        title: "Make a control group",
        copy: &[
            Para::Each {
                mouse: "A control group saves a selection so you can select it again with one [key. Select your Lancer and press {assign}. This saves it as group 1.|click. Select your Lancer, then click the plus above the minimap to save it as a group.]",
                touch: "A control group saves a selection so you can select it again with one tap. Select your Lancer, then tap the plus above the minimap[ or press {assign}] to save it as a group.",
                pad: "A control group saves a selection so you can select it again quickly. Select your Lancer and press D-pad right. Point the left stick at Group 1 and press {X} to save it there.",
            },
            Para::Each {
                mouse: "[Press {recall} to select group 1 again. Press it twice to move the camera to it. You can also click the plus above the minimap to save a group, and click the group there to select it.|Click the group above the minimap to select it again.]",
                touch: "Tap the group above the minimap[ or press {recall}] to select it again.",
                pad: "Press D-pad right and pick Group 1 with {A} to select it again.",
            },
        ],
        goal: Goal::Group,
    },
];

pub(super) const BUILD_OUTRO: &str = "";

const RAID: &str = "Five enemy Scuttlers are coming for your Harvesters. Scuttlers are fast \
    and cheap and kill Harvesters quickly, but a Sentinel beats a Scuttler one on one.";
const HUNTING: &str = "Hunting units stop to chase and fight every enemy they meet.";
const ADVANCE: Para = Para::Each {
    mouse: "A right-click on the ground is an Advance instead. Advancing units fire at enemies in range but never stop or chase, so a raid can slip past them.",
    touch: "A long-press on the ground is an Advance instead. Advancing units fire at enemies in range but never stop or chase, so a raid can slip past them.",
    pad: "Pressing {X} on the ground is an Advance instead. Advancing units fire at enemies in range but never stop or chase, so a raid can slip past them.",
};
const ROCK: &str = "Two enemy Lancers stand behind the rock line in the middle. Rock blocks \
    shots between ground units but not sight, so up close you can see them but can't hit \
    them through it.";
const AROUND: &str = "A Lancer can hit targets 5.5 tiles away, more than twice as far as a \
    Sentinel, and kills a Sentinel with one shot. Send your fast Scuttlers around the end of the \
    rock to reach them, with the rest of your army behind.";
const LEVELS: &str = "Three enemy Sentinels hold the high ground in the northeast. Ground \
    units can only shoot units on their own level, so neither side can shoot across \
    the cliff.";
const RAMP: &str = "A ramp at the west end of the cliff joins both levels, and units on it \
    can shoot up and down. Send your army up the ramp to fight.";
const WARDEN: &str = "An enemy Warden is heading for your base. It has 340 HP, more than \
    five Sentinels together.";
const BESIDE_TURRETS: &str = "Running units move without stopping to fight. Then fight the \
    Warden beside your Turrets.";
const OUTPOST: &str = "The enemy outpost in the east has a Fabricator and a Turret. Destroy both.";

pub(super) const FIGHT_BRIEFING: &str = "Five enemy Scuttlers are coming for your Harvesters. \
    Stop them with a Hunt order. Next, beat two Lancers behind a rock line and three Sentinels up \
    a ramp. When a Warden heads for your base, fall back to your Turrets with a Run order and \
    fight it there. Then destroy the enemy outpost.";

pub(super) const FIGHT: [Step; 5] = [
    Step {
        title: "Stop the raid",
        copy: &[
            Para::Same(RAID),
            Para::Each {
                mouse: "Select your army, [press {hunt}|click Hunt in the bottom panel], and click near your Harvesters.",
                touch: "Select your army, tap Hunt in the bottom panel[ or press {hunt}], and tap near your Harvesters.",
                pad: "Press {LB} to select your army, then press {Y} and pick Hunt. Press {A} near your Harvesters.",
            },
            Para::Same(HUNTING),
            ADVANCE,
        ],
        goal: Goal::Defeat(0),
    },
    Step {
        title: "Get around the rocks",
        copy: &[Para::Same(ROCK), Para::Same(AROUND)],
        goal: Goal::Defeat(1),
    },
    Step {
        title: "Take the ramp",
        copy: &[Para::Same(LEVELS), Para::Same(RAMP)],
        goal: Goal::Defeat(2),
    },
    Step {
        title: "Fall back",
        copy: &[
            Para::Same(WARDEN),
            Para::Each {
                mouse: "Select your army, [press {run}|click Run in the bottom panel], and click near your Turrets.",
                touch: "Select your army, tap Run in the bottom panel[ or press {run}], and tap near your Turrets.",
                pad: "Press {LB} to select your army, then press {Y} and pick Run. Press {A} near your Turrets.",
            },
            Para::Same(BESIDE_TURRETS),
        ],
        goal: Goal::Defeat(3),
    },
    Step {
        title: "Destroy the outpost",
        copy: &[
            Para::Same(OUTPOST),
            Para::Each {
                mouse: "[Press {hunt}|Click Hunt] and click near the outpost to clear the way, then right-click each structure to attack it.",
                touch: "Tap Hunt[ or press {hunt}] and tap near the outpost to clear the way, then long-press each structure to attack it.",
                pad: "Press {Y}, pick Hunt, and press {A} near the outpost to clear the way, then press {X} on each structure to attack it.",
            },
        ],
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

pub(super) const FIGHT_OUTRO: &str = "";

const FOG: &str = "Ground you have never seen is dark. Ground you have seen but can't see \
    right now is dimmed. It shows enemy structures as you last saw them, but not enemy units.";
const SURVEYOR: &str = "The Surveyor is your fastest ground unit and sees 9 tiles, but it \
    has no weapons. An enemy base is somewhere to the east.";
const SCOUT_SAFE: &str = "A scouting Surveyor stays out of reach of enemies it has seen.";
const SURVEYOR_RADAR: &str = "A Surveyor that stands still for 2 seconds also unfolds a \
    short-range radar. Enemies up to 12 tiles away that it can't see show up as blips.";
const ARRAY: &str = "An Array is a structure with a long-range radar. It sees 9 tiles around \
    itself and shows enemies up to 20 tiles away as blips.";
const BLIPS: &str = "A blip shows where an enemy is, but not what it is. A full diamond means it \
    is on the ground, and only the top half of a diamond means it is in the air.";
const AIRCRAFT: &str = "Two Buzzards wait at the enemy base. Buzzards are aircraft. They fly \
    over rock and attack only targets on the ground.";
const ANTI_AIR: &str = "Most ground units can't shoot aircraft. Flak Turrets and Flakhounds \
    shoot only aircraft. Sentinels can shoot both, but are weak against aircraft.";
const INBOUND: &str = "The Buzzards are flying to your base. Watch for their blips on your \
    Array's radar before they come into sight.";
const FLAK_FIRST: &str = "Your Flak Turret hits them from 5.5 tiles away. A Buzzard must come \
    within 3 tiles to fire, so the turret shoots first.";
const IN_HARMS_WAY: &str = "Buzzards attack units before structures, so Harvesters left idle \
    beside your new structures are in danger.";
const WELDING: &str = "Harvesters repair damaged structures and ground units back to full health. Repairing costs scrap for each point of health it restores.";
const IDLE_WELD: &str = "Idle Harvesters also repair damage near them on their own.";
const REPAIR_BAY: &str = "A Repair Bay repairs your units within 4 tiles of it, up to four \
    at a time. It works quickly once they are out of combat. It also repairs nearby structures \
    slowly, and pays for all repairs with scrap.";
const WRECKS: &str = "A destroyed unit leaves a wreck that holds some of the scrap the unit cost.";
const WRECKS_FADE: &str = "Wrecks deteriorate over time, so collect them soon.";

pub(super) const SUSTAIN_BRIEFING: &str = "An enemy base is somewhere to the east. Find it \
    with your Surveyor, and build an Array to see enemies on radar. The base has Buzzards, which \
    are aircraft, so build a Flak Turret before they attack. Then repair the damage, build a \
    Repair Bay, and collect the wrecks from the fight.";

pub(super) const SUSTAIN: [Step; 7] = [
    Step {
        title: "Scout the east",
        copy: &[
            Para::Same(FOG),
            Para::Same(SURVEYOR),
            Para::Pointer {
                pointer: "Select the Surveyor and {right-click} far to the east to scout there.",
                pad: "Select the Surveyor, move the reticle far to the east, and press {X} to scout there.",
            },
            Para::Same(SCOUT_SAFE),
            Para::Same(SURVEYOR_RADAR),
        ],
        goal: Goal::Find,
    },
    Step {
        title: "Build an Array",
        copy: &[
            Para::Same(ARRAY),
            Para::Same(BLIPS),
            Para::Each {
                mouse: "Select a Harvester, click Build[ or press {build}], and choose Array in the Utility column. Click open ground east of your Foundry to place it.",
                touch: "Select a Harvester, tap Build[ or press {build}], and choose Array in the Utility column. Drag its outline east of your Foundry and tap the check mark.",
                pad: "Select a Harvester, press {Y}, and pick Build, then Utility, then Array. Press {A} on open ground east of your Foundry to place it.",
            },
        ],
        goal: Goal::Build(BuildingKind::Array, 1),
    },
    Step {
        title: "Build a Flak Turret",
        copy: &[
            Para::Same(AIRCRAFT),
            Para::Same(ANTI_AIR),
            Para::Each {
                mouse: "Select a Harvester, click Build[ or press {build}], and choose Flak Turret in the Defense column. Click open ground east of your Foundry to place it.",
                touch: "Select a Harvester, tap Build[ or press {build}], and choose Flak Turret in the Defense column. Drag its outline east of your Foundry and tap the check mark.",
                pad: "Select a Harvester, press {Y}, and pick Build, then Defense, then Flak Turret. Press {A} on open ground east of your Foundry to place it.",
            },
        ],
        goal: Goal::Build(BuildingKind::FlakTurret, 1),
    },
    Step {
        title: "Shoot down the Buzzards",
        copy: &[
            Para::Same(INBOUND),
            Para::Same(FLAK_FIRST),
            Para::Same(IN_HARMS_WAY),
            IDLE_PICK,
        ],
        goal: Goal::Defeat(0),
    },
    Step {
        title: "Repair the damage",
        copy: &[
            Para::Same(WELDING),
            Para::Same(
                "Select a Harvester and {right-click} a damaged structure or unit to repair it.",
            ),
            Para::Same(IDLE_WELD),
        ],
        goal: Goal::Weld,
    },
    Step {
        title: "Build a Repair Bay",
        copy: &[
            Para::Same(REPAIR_BAY),
            Para::Each {
                mouse: "Select a Harvester, click Build[ or press {build}], and choose Repair Bay in the Utility column. Click open ground next to your Foundry to place it.",
                touch: "Select a Harvester, tap Build[ or press {build}], and choose Repair Bay in the Utility column. Drag its outline next to your Foundry and tap the check mark.",
                pad: "Select a Harvester, press {Y}, and pick Build, then Utility, then Repair Bay. Press {A} on open ground next to your Foundry to place it.",
            },
        ],
        goal: Goal::Build(BuildingKind::RepairBay, 1),
    },
    Step {
        title: "Collect the wrecks",
        copy: &[
            Para::Same(WRECKS),
            Para::Same(
                "Select a Harvester and {right-click} a wreck to collect it, the same way as a scrap pile.",
            ),
            Para::Same(WRECKS_FADE),
        ],
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

pub(super) const SUSTAIN_OUTRO: &str = "That completes the lessons.";
