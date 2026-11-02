//! Frame-exact camera scripts for native frame profiling.
//!
//! A script advances every frame by [`FRAME_SECONDS`] whatever the display
//! does, so the same frames get drawn on any machine and each carries a 120 Hz
//! frame's share of work.

use crate::camera::Camera;
use crate::numeric::to_f32;
use aftermine_protocol::{CameraMotion, FrameScript};
use macroquad::prelude::{Vec2, vec2};

/// How much time each scripted frame advances.
pub(crate) const FRAME_SECONDS: f32 = 1.0 / 120.0;

/// What a script wants from the frame about to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// Draw it, but leave it out of the samples.
    Settle,
    /// Draw it and keep its sample.
    Measure,
    /// The script has finished; this frame is not part of it.
    Done,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// Next frame frames the camera and starts settling.
    Start,
    /// Letting caches catch up before measuring begins.
    Settling {
        left: u32,
    },
    /// Measuring a fixed number of frames.
    Measuring {
        left: u32,
    },
    /// Measuring a zoom glide until it lands.
    Gliding,
    /// Measuring what follows a landed zoom glide.
    Resting {
        left: u32,
    },
    Done,
}

/// One script in progress.
#[derive(Debug, Clone)]
pub(crate) struct FrameScriptRun {
    settle_frames: u32,
    motion: CameraMotion,
    phase: Phase,
    /// Zoom glides still to begin, this one included.
    repeats_left: u32,
    /// Which way a pan is heading along the map's width.
    heading: f32,
}

impl FrameScriptRun {
    pub(crate) fn new(script: &FrameScript) -> Self {
        let repeats_left = match script.motion {
            CameraMotion::Zoom { repeats, .. } => repeats,
            CameraMotion::None { .. } | CameraMotion::Hold { .. } | CameraMotion::Pan { .. } => 1,
        };
        Self {
            settle_frames: script.settle_frames,
            motion: script.motion,
            phase: Phase::Start,
            repeats_left,
            heading: 1.0,
        }
    }

    /// Moves `camera` for the frame about to run and says how to treat it.
    /// Call once per presented frame, before the camera's own update.
    pub(crate) fn step(&mut self, camera: &mut Camera) -> Step {
        loop {
            match self.phase {
                Phase::Start if self.repeats_left == 0 => self.phase = Phase::Done,
                Phase::Start => {
                    self.frame_start(camera);
                    self.phase = Phase::Settling {
                        left: self.settle_frames,
                    };
                }
                Phase::Settling { left: 0 } => self.begin_measuring(camera),
                Phase::Settling { left } => {
                    self.phase = Phase::Settling { left: left - 1 };
                    return Step::Settle;
                }
                Phase::Measuring { left: 0 } | Phase::Resting { left: 0 } => {
                    self.repeats_left -= 1;
                    self.phase = Phase::Start;
                }
                Phase::Measuring { left } => {
                    self.phase = Phase::Measuring { left: left - 1 };
                    self.drive(camera);
                    return Step::Measure;
                }
                Phase::Gliding if camera.zooming() => return Step::Measure,
                Phase::Gliding => {
                    self.phase = Phase::Resting {
                        left: self.rest_frames(),
                    };
                }
                Phase::Resting { left } => {
                    self.phase = Phase::Resting { left: left - 1 };
                    return Step::Measure;
                }
                Phase::Done => return Step::Done,
            }
        }
    }

    const fn rest_frames(&self) -> u32 {
        match self.motion {
            CameraMotion::Zoom { rest_frames, .. } => rest_frames,
            CameraMotion::None { .. } | CameraMotion::Hold { .. } | CameraMotion::Pan { .. } => 0,
        }
    }

    fn frame_start(&self, camera: &mut Camera) {
        match self.motion {
            CameraMotion::None { .. } => {}
            CameraMotion::Hold { center, zoom, .. } | CameraMotion::Pan { center, zoom, .. } => {
                camera.show(world_point(center), to_f32(zoom));
            }
            CameraMotion::Zoom { center, from, .. } => {
                camera.show(world_point(center), to_f32(from));
            }
        }
    }

    fn begin_measuring(&mut self, camera: &mut Camera) {
        self.phase = match self.motion {
            CameraMotion::None { frames }
            | CameraMotion::Hold { frames, .. }
            | CameraMotion::Pan { frames, .. } => Phase::Measuring { left: frames },
            CameraMotion::Zoom { to, .. } => {
                camera.glide_to(camera.viewport() * 0.5, to_f32(to));
                Phase::Gliding
            }
        };
    }

    /// What the camera does during each fixed-length measured frame.
    fn drive(&mut self, camera: &mut Camera) {
        if let CameraMotion::Pan {
            tiles_per_second, ..
        } = self.motion
        {
            let before = camera.center;
            let step = to_f32(tiles_per_second) * FRAME_SECONDS * self.heading;
            camera.pan(vec2(step, 0.0));
            if camera.center == before {
                self.heading = -self.heading;
                camera.pan(vec2(-step, 0.0));
            }
        }
    }
}

fn world_point([x, y]: [f64; 2]) -> Vec2 {
    vec2(to_f32(x), to_f32(y))
}

#[cfg(test)]
mod tests;
