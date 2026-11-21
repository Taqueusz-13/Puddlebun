# Puddlebun

A rain simulator for CachyOS. You sit at a window on the third floor over a wet neon street at night, with rain running down the glass and the city muffled behind it. There's also a rainy British residential street you can walk around in. Rain is simulated on the GPU, and every sound is synthesised from that simulation.

This is stage 2a. See [SPEC.md](SPEC.md) for the full plan and the review decisions.

## Build and run (CachyOS)

You need Rust, ALSA headers (sound goes through PipeWire's ALSA bridge) and a Vulkan driver. On CachyOS with an NVIDIA card the Vulkan driver comes with `nvidia-utils`.

```bash
sudo pacman -S --needed rustup base-devel alsa-lib pipewire-alsa vulkan-icd-loader
rustup default stable
```

Unzip the project somewhere, for example `~/Projects/puddlebun`, then:

```bash
cd ~/Projects/puddlebun
cargo run --release
```

The first build takes a minute or two. After that, `cargo run --release` starts it straight away.

## Controls

At the window:

| Key | Action |
|---|---|
| Mouse | Move your head a little (it drifts on its own when you stop) |
| Scroll, W / S | Lean in towards the glass, or sit back. Lean in to look down at the street |
| O | Open the window a crack, or close it |

Everywhere:

| Key | Action |
|---|---|
| Tab | Show or hide the control panel |
| 1 | The neon window |
| 2 | The high-rise window (stage 2b) |
| 3–8 | Street viewpoints: down the street, gutter, under the lamp, up high, bay windows, puddle mirror |
| N | Next place |
| F | Free-fly on the street (WASD, hold right mouse to look, Shift faster, Space / C up and down) |
| M | Mute |
| F11 | Fullscreen |
| F12 | Screenshot, saved to `~/Pictures/Puddlebun/` |
| Esc | Leave free-fly or fullscreen |
| Ctrl+Q | Quit |

The camera stays where you put it. On the street it drifts slowly back and forth over 40 seconds. If you want it to move on by itself, tick "Move to the next place by itself" in the panel and set the interval.

## Settings

Everything you change in the panel is saved automatically to `~/.config/puddlebun/settings.toml`. To edit the file by hand, close the app first, then:

```bash
kate ~/.config/puddlebun/settings.toml
```

If you break something, delete the file or start with `cargo run --release -- --reset-settings`.

## What stage 2a adds

The window. A small room on the third floor: a sash window with a stone sill outside and a wooden one inside, the Puddlebun plush on the sill and an old radiator underneath. Across the road is a New York style street with fire escapes, awnings, shop fronts and neon signs (mostly English, a couple in Japanese). The room is lit by a low lamp behind you, and the neon spills in through the glass.

The glass. Raindrops hit the panes where the wind drives them, sit, grow and merge. Once a drop is heavy enough it runs, wobbling, sticking on rough patches, following the wet trails of earlier runners and leaving beads behind. Each drop works as a tiny lens that glows with the lights behind it. A fine spray haze builds up and the runners wipe clear paths through it. Condensation creeps in from the edges of each pane (there's a slider for it).

Opening the window. Press O and the lower sash sticks for a moment, then slides up about 10 cm. Rain spatters onto the inner sill, which darkens and dries again slowly, the condensation clears, and the city gets louder. Water also drips from the window head onto the stone sill and from the sill onto the fire escape below.

Sound. The audio was rebuilt from the ground up after the stage 1 review, when it was too thin.

- The distant rain is several layers: a pink-noise wash, a low roar for body, thousands of small taps on different materials arriving in clumps, awnings drumming, far-off metal pings, puddle plinks, and a drainpipe gurgling as it fills. Slow random drift and gust swells keep it changing.
- Nearby drops ring with the thing they hit. The fire escape goes "tink", glass taps have the pane's low thump, drips onto the stone sill "plink", and bigger drops on the ground carry a little low-end weight.
- Inside, the street comes through the glass: low-passed, with a slight boom in the low end, narrowed to a source in front of you. Open the window and it leaks in through the crack. The "window muffle" slider sets how thick the glass sounds.
- The room has its own small reverb, a radiator that ticks and occasionally hisses or clunks, and the sash scrapes when it moves.
- City life comes and goes: wet tyres hissing past, the el-train rumbling over its joints, a siren far off, the odd horn, neon buzz and the city's hum. The "city life" slider sets how much. On the residential street you only get the occasional car.

The look. A "stylise" slider goes from the stage 1 realistic render to a lo-fi painted look: soft painterly smoothing, a warm grade, gentle ink edges, slight banding, paper texture and grain. It's at 0.7 by default.

Stage 1 is all still there: the residential street, Marshall–Palmer drop sizes at terminal velocity, wind lag, puddle ripples, lamp fog, and the stage 1 tap and bubble voices.

## Headless mode (no window)

Useful for making stills or audio clips:

```bash
cargo run --release -- --screenshot window.png --size 2560x1440 --frames 90
cargo run --release -- --screenshot street.png --shot 2 --stylise 0
cargo run --release -- --audio window.wav --seconds 30 --rain 12 --open
cargo run --release -- --help
```

## Troubleshooting

- No sound. Check that `pipewire-alsa` is installed. The app says "no audio device" in the panel footer if it can't open one.
- Black window or crash on start. Run `RUST_LOG=puddlebun=debug,wgpu=warn cargo run --release` and send me the output.
- Low frame rate. Lower "max drops" and "reflection resolution" under Quality.

## Project layout

```
src/
  main.rs           command line, entry point
  app.rs            window, input, frame pacing, UI + audio glue
  settings.rs       saved settings
  weather.rs        rain physics (drop sizes, fall speeds, wind, living weather) + tests
  camera.rs         viewpoints, drift and free-fly
  scene.rs          procedural street geometry, lamps, windows
  city.rs           the neon street and the room by the window
  glass.rs          raindrops on the window panes + tests
  room.rs           the sash, drips, spatter, wet sill
  render/           GPU: renderer, rain, ripples, glass, post (bloom, tonemap, stylise), readback
  audio/            synthesiser: voices, rain bed, city life, room sounds, reverbs, output
  headless.rs       offscreen screenshots and audio renders
  shaders/          WGSL: world, street materials, rain, ripples, post
```
