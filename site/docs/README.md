# Synkopa user guide

The guide is plain HTML and CSS, published as part of the existing GitHub Pages site.
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

Keep screenshots in `images/` and update the image dimensions, alt text and caption when
replacing them.

The first-song pattern uses MP3 copies of the included Kick, Clap and Hat sounds from
`crates/engine/library`. See the repository’s `THIRD_PARTY_NOTICES.md` for sample credits.

Plugin images are WebP copies of the app screenshots in `site/img`, except
`plugin-master.webp`, `plugin-analyzer.webp` and `plugin-warp.webp`, which are
browser-mode captures of the actual frontend at `aa7215f` with mock demo data.
The two Delay listening clips illustrate a dry hit and timed repeats. They come from
the bundled `rimshot-808.flac` with an offline echo applied to the second clip.
