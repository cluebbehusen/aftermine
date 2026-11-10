# Icon sources

`tools/gen_icon.py` builds every shipped icon and Top Shelf image from these
files. Each is approved art; replace one only with art that has been approved
again.

- `aftermine.png`: the 1024 square app icon art. The iPad, Mac and desktop icons
  come from it.
- `desktop_mask.png`: the rounded silhouette for desktop and Mac icons.
- `tv_icon_front.png` and `tv_icon_back.png`: the same art upscaled to 4096 with
  Real-ESRGAN (`RealESRGAN_x4plus`), then split into the Scuttler with its rock
  and the sky behind them. Stacked, they reproduce the upscale exactly. The
  Apple TV icon is a 5:3 band of them.
- `top_shelf_scene.png`: the Top Shelf scene without text, a real frame of a bot
  match on Old Workings captured from the Mac shell in a 2400 by 1200 window
  (4800 pixels wide), the camera centred on tile (27.5, 37) at zoom 42.3, just
  before the warhead reaches the island. It keeps the capture's rows 475 to
  1965, clear of the HUD.
- `top_shelf_scene.json`: that match as a replay, including the commands that
  staged the scene. Replays reproduce only on the build that recorded them, so
  check out the commit in its `meta.build` in a worktree before changing it.
  Resume it with `Aftermine --replay <file> --debug-server --paused`, cut at the
  tick you want to change, and capture with `aftermine-driver live`.
