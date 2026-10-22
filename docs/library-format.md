# Libraries

A library is a folder of lineups: `library.json`, `manifest.json`, and the clips and pictures
beside them. The format, and a tool that checks a library and publishes it to the web, are in the
[arcwise-library](https://github.com/265866/arcwise-library) repository:
[docs/format.md](https://github.com/265866/arcwise-library/blob/main/docs/format.md).

## How Arcwise uses them

The settings list the libraries Arcwise shows: https://library.arcwise.lol/ to begin with, and
any web address or folder added since. The list is kept in `%APPDATA%\Arcwise\settings.json`
under `libraries`; until it is first changed, a folder chosen in an older Arcwise is added after
https://library.arcwise.lol/.

- Lineups from every library ticked show together. A map or agent several libraries list shows
  once, as the first of them names it, and so does an ability of the same agent with the same
  name. When two libraries list the same lineup id, the lineup comes from the one higher in the
  list, and the settings say how many of the lower one's lineups are hidden that way.
- A library on the web is mirrored in `%LOCALAPPDATA%\Arcwise\libraries\<host and path>`.
  Arcwise fetches its `library.json` and `manifest.json` each time it reads the libraries, and
  drops its copies of files whose SHA-256 the manifest says changed. A picture is fetched into
  the mirror when first shown. A clip streams from the web while it is fetched into the mirror,
  and plays from the mirror after. Each file is asked for at `<path>?v=<hash>` and checked against
  the manifest once fetched. A file the manifest does not list is not asked for.
- Without a connection, or when what the web sends is not a library or a manifest, a library on
  the web shows as it was last fetched, and whatever is in its mirror plays.
- **Offline…** in the settings fetches the files of the maps chosen, or of every map, into the
  mirror, four at a time, resuming a file where an attempt stopped. Each time the libraries are
  read, Arcwise fetches what those maps are missing, which after a change to the library is what
  changed.
- A folder is read as it is; nothing is copied.
