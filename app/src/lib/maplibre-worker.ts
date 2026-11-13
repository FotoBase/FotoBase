// MapLibre GL JS v6 resolves its worker relative to `import.meta.url`, but
// Vite bundles maplibre into a hashed chunk and never emits the worker file,
// so the default resolution 404s and clustering/GeoJSON processing never
// starts (every `querySourceFeatures` stays empty). Emit the worker as a
// Vite asset and point MapLibre at it once — the migration guide's bundler
// recipe (docs: v5-to-v6 migration guide, setWorkerUrl() is bundler-only).
import workerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?url";

import { setWorkerUrl } from "maplibre-gl";

setWorkerUrl(workerUrl);
