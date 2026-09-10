# Library format

Arcwise reads lineups from a folder on disk, the library. It never downloads anything. This page
describes version 1 of the layout.

```
library/
  library.json
  maps/
    <map id>.webp              card image for the map chooser, about 5:2
    <map id>-attack.svg        overhead map, attack side
    <map id>-defense.svg       overhead map, defense side
  agents/
    <agent id>.webp            square portrait
  abilities/
    <ability id>.webp          square icon
  lineups/
    <lineup id>.mp4            the clip
    <lineup id>-start.webp     its first frame: the view from the stand spot
    <lineup id>-end.webp       its last frame: where the utility landed
    <lineup id>-aim.webp       optional: the crosshair at the moment of the throw
```

Overhead maps are square, and every position in `library.json` is a percentage of the map's
width (`x`) and height (`y`).

A missing image shows a placeholder.

## library.json

```json
{
  "format": 1,
  "maps": [{ "id": "7eaecc1b-…", "name": "Ascent" }],
  "agents": [
    {
      "id": "320b2a48-…",
      "name": "Sova",
      "role": "initiator",
      "color": "#3D8BFF",
      "abilities": [{ "id": 10, "name": "Shock Bolt", "slot": 1 }]
    }
  ],
  "lineups": [
    {
      "id": 211,
      "map": "7eaecc1b-…",
      "agent": "320b2a48-…",
      "ability": 10,
      "side": "attack",
      "title": "Double shocks for A main",
      "tags": ["essential"],
      "stand": { "x": 20.5, "y": 80.25, "callout": "A Lobby" },
      "path": [[35.1, 60.0], [50.2, 50.4]],
      "land": { "x": 50.2, "y": 50.4, "callout": "A Main" },
      "steps": {
        "setup": ["Charge 2 bars, 1 bounce"],
        "aim": "Top of the antenna",
        "release": "Left click",
        "note": null
      }
    }
  ]
}
```

| Field | Meaning |
| --- | --- |
| `role` | `duelist`, `initiator`, `controller` or `sentinel`; groups the agent chooser. |
| `color` | `#rrggbb`; tints the markers and highlights for that agent. |
| `slot` | The ability's place on the ability bar, 1 to 4; abilities are listed in this order. |
| `side` | `attack` or `defense`. |
| `tags` | Any of `essential` and `pro`; other values are ignored. |
| `path` | The throw's arc from the stand spot, drawn on the map. May be empty. |
| `callout` | The place name, or `null` when it has none. |
| `steps` | How to throw, or `null`. `note` is shown over the clip when the throw needs more than a plain click, such as a jump. |

Arcwise refuses a library whose lineups refer to a map, agent or ability it does not list, or that
lists a lineup id twice.
