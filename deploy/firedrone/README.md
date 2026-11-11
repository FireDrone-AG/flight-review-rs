# FireDrone build

This fork of [PX4/flight-review-rs](https://github.com/PX4/flight-review-rs)
builds the image for logs.firedrone.com. FireDrone changes are kept small so
upstream merges stay easy:

| Change | Why |
| --- | --- |
| `deploy/firedrone/Dockerfile` (+ `.dockerignore`) | Server with `--features s3` plus the built web app, non-root, `sqlite3` for backups. The upstream `Dockerfile` is untouched. |
| `crates/server/src/main.rs`: `--static-dir` / `FLIGHT_REVIEW_STATIC_DIR` | Lets the server serve the web app (unknown paths get `index.html`). Off unless set. Candidate for upstream. |
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
