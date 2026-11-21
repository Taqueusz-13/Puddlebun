# Puddlebun — agreed spec

Decided with the owner on 20 Nov 2026. Each later stage builds on this.

## Platform and use

- **Stack:** Rust + wgpu + winit (egui only as the temporary stage-1 panel).
- **Target:** CachyOS only, KDE Plasma on Wayland. The PC has an RTX 5070 and a 2560×1440 165 Hz monitor, and that's the only monitor.
- **Main use:** an ambient window left open while working or gaming. A **Plasma live wallpaper** comes later (layer-shell, Plasma only), muted by default with a toggle.
- **In the background:** about 30 fps when unfocused, stop rendering when hidden. While **any fullscreen window** is open (detected with a KWin script), pause everything, **including audio**.
- **Install:** a script that builds it and adds a launcher entry. No autostart. Settings save automatically.
- **Code ownership:** the owner just wants the app and asks Claude for changes.

## Look

- **Cozy and stylised:** lo-fi warmth inside, neon city outside. A **"stylise" slider** runs from realistic (the stage 1 look) to full lo-fi/neon and applies everywhere; it defaults to mostly stylised. Keep the visual clarity of stage 1.
- Fully procedural (no downloaded assets). Physics is balanced: real distributions and speeds, tuned where it looks better.
- **Motion:** a happy medium where everything feels physical.
- **Colour:** adaptive. The panel picks up the scene's light (sodium in residential, neon in the city, cold blue in the alley, lightning flashes), with **synthwave accents**.
- **GPU:** a stable high-end base (reflections, volumetric fog, bloom, motion blur, depth of field), plus an optional **RT toggle** for hardware ray-traced reflections that falls back gracefully. A stats scope shows performance.

## World

- **One connected night world with set spots and smooth camera glides** between them when you switch (it stays put otherwise). Entering an interior ends with a rain-blur fade.
  - **Residential:** British terraced street + **house window** interior (indoor acoustics: muffled outside, crisp taps on the glass).
  - **City:** shopfronts and neon, traffic on by default, plus a **parked car** interior (rain drumming on the roof).
  - **Alley:** brick walls, one cold lamp, a sheltered doorway.
- **Camera:** drift between presets by default; a key switches to free-fly.
- **Weather:** living weather (presets driven by slow random modulators) and static, switchable. Lightning with distance-delayed thunder.
- **Traffic:** cars hiss past with headlights. On in the city, off elsewhere by default.
- **Interaction:** wipe the window and umbrella/shelter, both **off by default**.
- **Easter eggs:** small ones, including Puddlebun the bunny sheltering in the world.

## Audio

Fully procedural and stereo-positional:

- Nearby impacts are voiced individually from the simulation, with surface-dependent synthesis.
- Distant rain is a statistical bed driven by the same physics.
- An FX section sits on top: timbre, filters, room, width.
- Procedural UI sounds have their own mute.
- Output goes to the PipeWire default device.

## Panels

- **Synth panel:** a separate window plus a tray icon. A pane of frosted glass showing live refracted rain, with real drops flowing around physical, weighted knobs and faders. Touches send ripples through it.
  - **Modulation:** LFOs, random/drift and envelopes, routed through a mod matrix.
  - **Presets:** "living weather" is one of them.
  - **No MIDI.**
  - **Controls:** every world, weather and audio parameter:
    - time of night
    - lamps
    - fog
    - wetness and puddles
    - wind
    - lightning, plus a manual strike
    - traffic
    - window condensation
- **Node view:** tabs for the sim/render pipeline, the audio graph and the modulation graph. Live thumbnails and waveforms read from the real engine. Click a node to view its output on its own (solo) or switch it off (bypass).

## Name

**Puddlebun**, a bunny and rain theme.

## Review of stage 1 (20 Nov 2026)

What the owner reported after running it:
- **Performance:** fps totally fine.
- **Rain visuals:** clear and pleasing, no shimmer or flicker; could be more varied.
- **Audio:** the biggest weakness. Too thin, lacks body ("piddly") and doesn't sound varied.
- **Puddles:** in the wrong places and the wrong shapes; should be more visible.

Decisions:
- **Window view is now the heart of the project.** A cosy look out of a window at night over a city (New York bones with Night City scale and neon), muffled rain and streaks running down the glass.
- **Two city windows, switchable:**
  - mid-height flat looking across a neon street
  - high-rise view over the towers
- **Head behind the glass:** the mouse moves your head about ±0.5 m with parallax; scroll or W/S leans in towards the glass; a subtle idle sway when untouched.
- **Room edge:** a radiator under the window (faint tick) and a Puddlebun plush on the sill for now. Desk lamp, plant, mug, curtains and fairy lights come later.
- **Glass:**
  - drops bead, slide and streak; condensation builds at the edges
  - an animated **"open a crack" toggle** (key + panel button): louder, crisper rain, a cold-air whoosh, the curtain moving, and rain occasionally spattering onto the sill
- **Indoor audio:**
  - moderately muffled by default, with a **muffle slider**
  - sparse city life (tyre hiss, distant siren, elevated train rumble, neon buzz) with a **"city life" slider** that scales how much is going on
- **Audio fixes, everywhere:**
  - more sources: drips from gutters and sills, drainpipes, rain on metal, awnings and car roofs
  - change over time: swells, gusts, lulls
  - real low-end body and room tone
- **Rain variety:**
  - visible gust sheets
  - a wider mix of drop sizes and brightness, never unrealistically drastic
  - drips and streams falling from edges
  - more noticeable wind slant
- **Puddles:** sensible placement (gutters, drains, real low spots) and believable shapes, crisper and more visible.
- **Camera:** drift passes take about 20 s (was 80 s). It **stays put at a spot until you switch**, both outdoors and at windows.
- **Residential house window:** kept for later (stage 4).
- **Repo:** github.com/Taqueusz-13/Puddlebun

## Stages

1. ✅ **Engine core + residential street + rain physics + procedural audio + basic panel**
2. ✅ **2a:** mid-height neon street window, glass drop/streak sim, open-a-crack, indoor muffled audio, the audio body/variety fixes, stylise slider, faster drift
   **2b:** high-rise window view, puddle placement/shape fixes, rain variety (gust sheets, drips, slant)
3. Glass synth panel with modulation and presets + node view + stats + wallpaper mode + fullscreen pause + install script
4. Alley, residential house window, camera glides between spots, traffic, lightning and thunder, more room details
5. RT toggle, easter eggs, final polish

### Stage 1 notes for later stages

- The rain particle count is scaled to the budget (k ≈ 0.045 of real density). Per-drop brightness is boosted ×4 to compensate, so relative densities stay physical.
- The temporary egui panel (`src/ui.rs`) is replaced in stage 3.
- Hooks already in place:
  - `misc.w` = lightning flash (sky, fog)
  - `SURF_*` kinds shared by shaders and audio
  - `--cam` / `--shot` for headless renders

### Stage 2a notes for later stages

- Places are numbered "spots": 0 = neon window, 1 = high-rise window (2b, key 2 shows a "coming in 2b" note for now), 2–7 = the six street viewpoints. `camera::spot_scene`, `next_spot` and `spot_names` are the single source of truth.
- The neon window is on the third floor (sill at 8.07 m) so leaning in shows the street. Room and window geometry live in `city.rs` (`WindowGeom`, `build_room`), and the GPU collision heights for the outer sill and fire escape are `W_SILL_TOP` / `FE_TOP` in `world.wgsl`. Moving the window means moving all of those together.
- From the window the rain box is 20 m wide, between our facade and the one opposite, at 2.2× the street density (nothing further away is visible).
- Glass drops are drawn 1.35× their simulated size (`glass::VIS_SCALE`) so they read on screen. The simulation itself uses real sizes.
- Audio messages are `AudioMsg::Impact` (with `inside` = in the room, not heard through the glass) and `AudioMsg::Slide`. Audio-only surfaces: `SURF_WOOD` (inner sill), `SURF_STONE` (outer sill).
- Known for 2b: the city road seen from above looks grey and flat; it needs the puddle work and stronger wet reflections. Street drips from gutters and edges belong with "rain variety".
