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
