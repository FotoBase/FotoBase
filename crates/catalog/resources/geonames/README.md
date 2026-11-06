# GeoNames cities dataset (TASK-06-008)

`cities.txt` is extracted from the GeoNames *cities15000* dump
(<https://download.geonames.org/export/dump/>) — all cities with population
≥ 15 000 carrying a `P` feature class. Columns: `name`, `latitude`,
`longitude`, `country`, `geonameid`, sorted by name.

Licensed CC BY 4.0 (attribution-only, no weights or code); recorded in
`crates/ai/model-manifest.json` and pinned by SHA-256 there.

Regenerate:

```sh
curl -LO https://download.geonames.org/export/dump/cities15000.zip
unzip cities15000.zip
python3 - <<'EOF'
out = open("crates/catalog/resources/geonames/cities.txt", "w", encoding="utf-8")
for line in open("cities15000.txt", encoding="utf-8"):
    c = line.rstrip("\n").split("\t")
    if len(c) >= 15 and c[7].startswith("P"):
        out.write(f"{c[1]}\t{c[4]}\t{c[5]}\t{c[8]}\t{c[0]}\n")
out.close()
EOF
sort -t$'\t' -k1,1 -o crates/catalog/resources/geonames/cities.txt \
    crates/catalog/resources/geonames/cities.txt
shasum -a 256 crates/catalog/resources/geonames/cities.txt   # update manifest
```

Credit line required by CC BY 4.0 (shipped in the app's help/about text):
"Place names from GeoNames (geonames.org), licensed CC BY 4.0."
