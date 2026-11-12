# FireDrone build

This fork of [PX4/flight-review-rs](https://github.com/PX4/flight-review-rs)
builds the image for logs.firedrone.com. FireDrone changes are kept small so
upstream merges stay easy:

| Change | Why |
| --- | --- |
| `deploy/firedrone/Dockerfile` (+ `.dockerignore`) | Server with `--features s3` plus the built web app, non-root, `sqlite3` for backups. The upstream `Dockerfile` is untouched. |
| `--static-dir` / `FLIGHT_REVIEW_STATIC_DIR` (`main.rs`, `webapp.rs`) | Lets the server serve the web app (unknown paths get `index.html`). `index.html` and other files are revalidated on every load (`Cache-Control: no-cache`), hashed files under `/_app/immutable/` are cached for good, so a new version shows up without a forced reload. Off unless set. Candidate for upstream. |
| `--nav-link NAME=URL` / `FLIGHT_REVIEW_NAV_LINKS` (`webapp.rs`, `GET /api/nav-links`; `ToolLinks.svelte` in the sidebar, the mobile menu and the log viewer's top bar) | Links to other tools, e.g. FireDrone Fleet. Set at runtime, so no host names are in this repo or the image. None unless set. |
| Dark mode (`frontend/src/dark.css`, theme script in `app.html`, `utils/themeColors.ts` for the plot grids) | Upstream gives only the nav dark styles and makes `dark:` follow the system, so the toggle did nothing and pages stayed white. `dark.css` swaps the palette for all components when `<html>` has `dark` (saved choice, else the system's) and switches the `dark:` variant off. |
| SQLite in WAL mode, one transaction per log's rows (`db/sqlite.rs`) | Readers (the log list, FireDrone Fleet) no longer wait for uploads, which made the list time out while drones uploaded. |
| `--vehicle-name-header` / `FLIGHT_REVIEW_VEHICLE_NAME_HEADER` (`main.rs`, `lib.rs`, `api/upload.rs`) | Takes an upload's vehicle name from a header the reverse proxy sets after checking the drone's key (`X-Drone-Id`), so a drone can't file logs under another drone's name. Off unless set. |
| `vehicle_name` filter on `GET /api/logs` (`db/*.rs`) | Exact match, so FireDrone Fleet can list one drone's logs. |
| `.github/workflows/firedrone-image.yml` | On a tag `firedrone-vX.Y.Z`: build, smoke-test, push `ghcr.io/firedrone-ag/flight-review:X.Y.Z`. |

Deployment, settings, S3 key rotation and database backups are described in
`firedrone-fleet/deploy/LOGS.md`, together with the rest of FireDrone-VPS.

```bash
docker build -f deploy/firedrone/Dockerfile -t flight-review .      # local build
git fetch upstream && git merge upstream/main                       # upstream: https://github.com/PX4/flight-review-rs
git tag firedrone-v0.1.0 && git push origin firedrone-v0.1.0        # release
```

S3 settings come from `AWS_*` environment variables (`object_store`):
`AWS_ENDPOINT_URL`, `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`.
A custom endpoint is addressed path-style (`<endpoint>/<bucket>`).
