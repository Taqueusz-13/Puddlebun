# 🐰🌧 Puddlebun

A high-quality rain simulator for CachyOS: a photoreal rainy British street at night, GPU raindrop physics, live puddle ripples, and fully procedural, positional rain audio driven by the simulation.

**This is stage 1.** See [SPEC.md](SPEC.md) for the full plan and the review decisions.

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

| Key | Action |
|---|---|
| **Tab** | Show or hide the control panel |
| **F** | Free-fly camera on or off |
| WASD | Move (in free-fly) |
| Right mouse (hold) | Look around (in free-fly) |
| Shift | Move faster |
| Space / C | Up / down |
| **1–6** | Jump to a viewpoint: down the street, gutter, under the lamp, up high, bay windows, puddle mirror |
| **N** | Next viewpoint |
| **M** | Mute |
| **F11** | Fullscreen |
| **F12** | Screenshot, saved to `~/Pictures/Puddlebun/` |
| Esc | Leave free-fly or fullscreen |
| Ctrl+Q | Quit |

By default the camera drifts slowly around a viewpoint and glides to the next one every 10 minutes. You can change the interval under *Camera & look*.

## Settings

Everything you change in the panel is saved automatically to `~/.config/puddlebun/settings.toml`. To edit the file by hand, close the app first, then:

```bash
kate ~/.config/puddlebun/settings.toml
```

If you break something, delete the file or start with `cargo run --release -- --reset-settings`.

## What stage 1 does

**The street.** A procedurally built terraced street with:

- brick fronts and ground-floor bay windows
- sash windows that open onto little rooms (interior mapping): some lit, some with curtains, a few with a TV flickering
- slate roofs and chimney stacks with pots, garden walls with coping, and kerbs
- double yellow lines, gully grates and manhole covers
- sodium-style street lamps every 30 m

**Wet surfaces.**

- Porous materials darken when wet and everything gets glossier.
- Puddles collect in the gutters and in dips in the road, and in sunken paving slabs.
- Reflections come from a mirrored render of the street, stretched vertically by surface roughness, which gives the streaky lamp reflections of wet tarmac.
- Lamp highlights have a physical size, so they show up in windows and on wet walls.

**Rain physics, on the GPU, up to 1M drops.**

- Drop sizes follow the **Marshall–Palmer** distribution for the current rain rate, and respawned drops are flux-weighted so the population in the air stays correct.
- Each drop falls at its **terminal velocity** for its size (Atlas et al. 1973).
- Each drop lags gusts according to its size (drag time τ = v/g), so small drops swirl and big ones keep falling.
- Gust fronts travel through the rain downwind.
- Drops are drawn as motion-blurred streaks with energy-conserving brightness, lit by every lamp with a strong forward-scattering lobe. Like real rain, they glow when the light is behind them.
- Impacts throw up splash droplets (and a central jet on water).

**Puddle ripples.** A wave-equation simulation on a 10 m tile at 5 mm resolution:

- Every simulated drop that lands in a puddle starts a ripple ring.
- The countless smaller drops are added at the real physical rate.
- Beyond the simulated area, statistical ripples take over.

**Fog and sky.** Each lamp's light scattering in the misty air is solved analytically, with falling-rain shimmer inside the glow. The overcast sky is lit orange from below by the town.

**Procedural audio. No samples, all synthesised.**

- The loudest nearby impacts, read back from the GPU simulation, are voiced individually, with a sound that depends on the surface:
  - water: an impact plus, often, an entrained-air bubble (Minnaert resonance with rising pitch)
  - wet tarmac and paving: filtered taps plus spray ticks
  - slate roofs: a ringing tap
  - soil: a dull thud
- Every voice is placed in stereo with panning, a tiny interaural delay and head shadow.
- Distant rain is a statistical "bed" driven by the same rain rate, wind and puddle level, plus a street reverb.

**Background behaviour.** When the window isn't focused it drops to about 30 fps. When it's minimised or hidden it stops rendering, but the rain keeps sounding.

**Living weather.** Rain rate and wind drift slowly, and every so often a heavier shower builds and passes. Turn it off in the panel for steady rain.

## Headless mode (no window)

Useful for making stills or audio clips:

```bash
cargo run --release -- --screenshot shot.png --size 2560x1440 --frames 90 --shot 2
cargo run --release -- --audio rain.wav --seconds 30 --rain 12
cargo run --release -- --help
```

## Troubleshooting

- **No sound.** Check that `pipewire-alsa` is installed. The app says "no audio device" in the panel footer if it can't open one.
- **Black window or crash on start.** Run `RUST_LOG=puddlebun=debug,wgpu=warn cargo run --release` and send me the output.
- **Low frame rate.** Lower *max drops* and *reflection resolution* under *Quality*.

## Project layout

```
src/
  main.rs           command line, entry point
  app.rs            window, input, frame pacing, UI + audio glue
  settings.rs       saved settings
  weather.rs        rain physics (drop sizes, fall speeds, wind, living weather) + tests
  camera.rs         viewpoints, drift and free-fly
  scene.rs          procedural street geometry, lamps, windows
  render/           GPU: renderer, rain, ripples, bloom/tonemap, readback
  audio/            synthesiser: voices, distant-rain bed, reverb, output
  headless.rs       offscreen screenshots and audio renders
  shaders/          WGSL: world, street materials, rain, ripples, post
```
