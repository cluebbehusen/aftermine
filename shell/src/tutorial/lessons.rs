//! What each lesson says and what completes each of its steps. Paragraphs
//! wrap to the card; `{token}`s name the player's current bindings.

use super::{Copy, Goal, Step};
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
