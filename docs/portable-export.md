# Portable export of your data — format v1

Fotobase keeps your user data in open places: XMP sidecars next to your photos, and `albums.json` next to the catalog. Some of it lives in Fotobase-specific fields (`fb:Albums`, `fb:Recipe`) that other tools don't read. The **portable export** writes all of it to plain files you can open without Fotobase, whenever you like (PLAN §5, "Portability guarantee").

**Options → Export my data…** lets you choose:

- **What:** albums, tags, edit recipes (any combination).
- **Format:** JSON (everything, versioned) or CSV (one table per section).
- **Where:** an existing folder. Each export gets a time-stamped name such as `fotobase-export-20261029-160523.json`, so an earlier export is never overwritten.

The export only reads your library. It never changes photos, sidecars or the catalog, and it never uses the network.

## JSON

One self-describing document validated by [`portable-export-v1.schema.json`](portable-export-v1.schema.json). Here is a complete example: [`portable-export-v1.example.json`](portable-export-v1.example.json).

| Field                | Meaning                                                                                                                                                                                                         |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `spec`, `version`    | Always `"fotobase.export"` and `1` for this format.                                                                                                                                                             |
| `exported_at`        | UTC time of the export.                                                                                                                                                                                         |
| `generator`          | The Fotobase version that wrote the file.                                                                                                                                                                       |
| `albums`             | `id` (the UUID stored in each member's `fb:Albums`), `name`, optional `date` / `place` / `description`, and `members` — photo paths in album order.                                                             |
| `tags`               | `name` and the `files` carrying that tag.                                                                                                                                                                       |
| `recipes`            | `file` and its `recipe`. Each recipe is a complete packet that conforms to the [edit-recipe spec v1](edit-recipe-spec-v1.md) and validates on its own against [`recipe-v1.schema.json`](recipe-v1.schema.json). |
| `unreadable_recipes` | Present only when some `fb:Recipe` values are not valid v1 recipes, for example ones written by a newer tool. Each one is kept verbatim (`raw`) with a `reason`, so nothing is lost.                            |

A section appears only if you picked it. An empty list means you picked it but it had no data.

## CSV

One RFC 4180 file per picked section: UTF-8, CRLF line endings, a header row, and fields quoted when needed.

| File            | Columns                                                                                                                                           |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| `…-albums.csv`  | `album_id, album_name, album_date, place, description, position, filename, path`. One row per member; an empty album gets one row with no member. |
| `…-tags.csv`    | `tag, filename, path`. One row per tagged photo.                                                                                                  |
| `…-recipes.csv` | `filename, path, recipe` (the recipe as compact JSON).                                                                                            |

CSV leaves out unreadable recipes. Use JSON for a complete copy.

## Checking your export outside Fotobase

Everything below runs without Fotobase installed.

```sh
# Validate the whole document, including every embedded recipe. The export
# schema references recipe-v1.schema.json relatively; point the base URI at
# a local copy of docs/ so it resolves offline (note the trailing slash):
pipx run check-jsonschema \
  --schemafile docs/portable-export-v1.schema.json \
  --base-uri "file://$PWD/docs/" \
  fotobase-export-20261029-160523.json

# Albums and how many photos each holds:
jq -r '.albums[] | "\(.name)\t\(.members | length)"' fotobase-export-20261029-160523.json

# Every photo tagged "sea":
jq -r '.tags[] | select(.name == "sea") | .files[]' fotobase-export-20261029-160523.json

# Validate one recipe on its own against the recipe spec:
jq '.recipes[0].recipe' fotobase-export-20261029-160523.json > recipe.json
pipx run check-jsonschema --schemafile docs/recipe-v1.schema.json recipe.json
```

You can also open the CSV files in any spreadsheet. The in-repo tests (`crates/catalog/tests/portable_export.rs`) run the same checks on every build: they validate the schema, read the JSON back into the same data, and validate the example above.
