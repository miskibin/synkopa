# Synkopa user guide

The guide uses static HTML and CSS with optional JavaScript, published as part of the existing GitHub Pages site.
Run `python3 -m http.server 8000 --directory site` from the repository root and open
`http://localhost:8000/docs/` to preview the landing-page link and guide together.

To serve the guide separately, use `site/docs` as the static root. It needs no build step,
dependencies, or absolute asset paths. On Vercel, use the Other framework preset and
select `site/docs` as the project root.

## Screenshots

- `overview.webp` and `mixer.webp` are desktop screenshots from the
  [Synkopa UX audit](https://github.com/miskibin/synkopa/pull/302) before the proposed changes.
- `rack.webp` shows the first-song drum pattern entered through Synkopa’s Channel Rack
  in browser development mode at `aa7215f`.
- `roll.webp` and `export.webp` show the actual Synkopa frontend with the built-in
  OBSIDIAN 2093 demo, captured using the repository's browser development mode at
  `aa7215f`. Development mode uses the browser mock backend.

Effects, instruments, library sounds, and recording steps use captures of the Synkopa
frontend at `a672f09` in browser development mode. The demo inputs, plugin values,
and meters shown in these captures illustrate the controls.

Keep screenshots in `images/`. Update the dimensions, alt text, pin coordinates,
and numbered instructions when replacing a screenshot. Coordinates are percentages
of the image width and height. Use `data-pin-side="below"` near the top edge so the
marker points up toward the control.

The first-song pattern uses MP3 copies of the included Kick, Clap and Hat sounds from
`crates/engine/library`. See the repository’s `THIRD_PARTY_NOTICES.md` for sample credits.

The two Delay listening clips illustrate a dry hit and timed repeats. They come from
the bundled `rimshot-808.flac` with an offline echo applied to the second clip.

## Icons

After installing the UI dependencies, run `node scripts/build-doc-icons.mjs` to
refresh the sprite from the app’s effect art and Channel Rack icons. The sampled
instrument thumbnails come from `ui/src/art/keys`. Two Channel Rack glyphs come from
[Font Awesome Free](https://fontawesome.com/license/free) under CC BY 4.0.
