# Epoch: FreeTAKServer Feature Parity

Date: 2026-01-29
Owner: Codex + User
Target: Full parity between `fts-rs` and Python FreeTAKServer

## How to use this file
- Update the parity matrix as work is completed.
- Use the Approval column to sign off on parity for each feature/area.
- Add links to concrete tests, fixtures, or endpoints once validated.

## Status legend
- NOT_STARTED
- IN_PROGRESS
- BLOCKED
- DONE

## Epoch 1: Basic Client Connectivity + Sharing (WinTAK/ATAK)

Definition: Clients can connect to the server and exchange GPS presence, points, and shapes (basic CoT relay).

Success criteria (user acceptance):
- [ ] WinTAK connects over TCP and shares position (PPLI/presence).
- [ ] ATAK connects over TCP and shares position.
- [ ] Points created on one client appear on the other.
- [ ] Shapes (lines/polygons) created on one client appear on the other.

Implementation checklist:
- DONE: CoT TCP listener (8087) and TLS listener (8089).
- DONE: Presence cache + send-on-connect.
- DONE: Broadcast relay for raw CoT XML to all clients.
- DONE: Disconnect CoT emitted on socket close.
- DONE: Add a connection/config guide for WinTAK/ATAK.
- TODO: Run a live connectivity test with WinTAK + ATAK.
- DONE: Minimal fileshare CoT broadcast for attachments on enterprise sync uploads (with broadcast=true).

### Connection Guide (Epoch 1)

Server (VPN): `10.8.0.0.1`

Recommended for Epoch 1 (authless):
- **Protocol**: TCP (not TLS)
- **CoT Port**: `8087`
- **Host/IP**: `10.8.0.0.1`

WinTAK:
1) Add a new server profile.
2) Set host to `10.8.0.0.1`, port `8087`, protocol TCP.
3) Connect and verify your position updates.

ATAK:
1) Add a network connection.
2) Host `10.8.0.0.1`, port `8087`, protocol TCP.
3) Connect and verify your position updates.

Optional (later):
- **TLS CoT**: `10.8.0.0.1:8089` (currently no client cert auth enforced).

## Parity Matrix (High Level)

| Area | Python FTS Reference | fts-rs Status | Notes / Next Actions | Approval |
| --- | --- | --- | --- | --- |
| Core CoT TCP service | `FreeTAKServer/services/tcp_cot_service` | NOT_STARTED | Implement full CoT handling + routing parity | |
| CoT over TLS (SSL CoT service) | `FreeTAKServer/services/ssl_cot_service` | NOT_STARTED | Rust TLS listener, cert mgmt, auth | |
| CoT parsing/serialization | `FreeTAKServer/components/core` + `FreeTAKServer/model` | NOT_STARTED | Full CoT domain model + XML mapping | |
| Presence + user/session tracking | `FreeTAKServer/model/User.py` + services | NOT_STARTED | Track connected users, presence propagation | |
| TCP data package service | `FreeTAKServer/core/services/TCPDataPackageService` | NOT_STARTED | Datapackage upload/notify parity | |
| SSL data package service | `FreeTAKServer/core/services/SSLDataPackageService` | NOT_STARTED | TLS datapackage service | |
| HTTP TAK API service | `FreeTAKServer/services/http_tak_api_service` | NOT_STARTED | Marti endpoints parity | |
| HTTPS TAK API service | `FreeTAKServer/services/https_tak_api_service` | NOT_STARTED | TLS Marti endpoints | |
| REST API service | `FreeTAKServer/services/rest_api_service` | NOT_STARTED | REST endpoints + auth | |
| Enterprise sync endpoints | `.../blueprints/enterprise_sync_blueprint.py` | NOT_STARTED | Upload/search/content/metadata | |
| Mission service + persistence | `FreeTAKServer/components/extended/mission` | NOT_STARTED | Missions CRUD, subscriptions, logs | |
| ExCheck service | `FreeTAKServer/components/extended/excheck` | NOT_STARTED | Task list and templates | |
| Federation (client/server) | `FreeTAKServer/components/extended/federation` | NOT_STARTED | Client/server federation parity | |
| Emergency / 911 | `FreeTAKServer/components/extended/emergency` | NOT_STARTED | Align schema + broadcast behavior | |
| File/image transfer | `FreeTAKServer/components/extended/files` | NOT_STARTED | File persistence + sharing | |
| XMPP chat | `FreeTAKServer/components/extended/xmpp_chat` | NOT_STARTED | Chat endpoints + storage | |
| Report/Track manager | `FreeTAKServer/components/extended/report`, `track_manager` | NOT_STARTED | Reporting + tracking parity | |
| KML generation | `FreeTAKServer/services/rest_api_service` | NOT_STARTED | KML export parity | |
| Configuration wizard | `FreeTAKServer/core/configuration` | NOT_STARTED | Rust config generator | |
| Certificate generation | `FreeTAKServer/core/util/certificate_generation.py` | NOT_STARTED | CA + user/server certs | |
| Logging/metrics | `FreeTAKServer/core/configuration/LoggingConstants.py` | NOT_STARTED | Structured logs + health | |
| CLI/service manager | `FreeTAKServer/controllers/services/FTS.py` | NOT_STARTED | CLI start/stop parity | |
| Persistence schema | `FreeTAKServer/core/persistence` | NOT_STARTED | DB schema + migrations | |

## Current Known fts-rs Delta (as of 2026-01-29)
- fts-rs implements only a small subset: CoT relay, minimal REST endpoints, SQLite persistence.
- Baseline build/runtime blockers in `fts-rs/src/main.rs` were fixed (broadcast tx ordering, TLS acceptor creation, HTTP server startup).
- Milestone 1 partial: broadcast now avoids echo to sender, and a basic presence cache is sent to new connections.

## Milestone 1 Progress

- DONE: TCP/TLS CoT sockets run, TLS acceptor configured.
- DONE: Broadcast messages include sender id to avoid echo.
- DONE: Presence cache (contact-bearing CoT) sent to new connections.
- DONE: Basic presence detection (contact or a-*) with callsign extraction.
- DONE: Connection state + disconnect cleanup removes presence from cache.
- DONE: Disconnect CoT broadcast on socket close (type t-x-d-d with link uid).
- TODO: Presence rules aligned to FTS (type taxonomy + special cases).

## Marti HTTP API Parity Progress
- DONE: Enterprise sync storage + endpoints implemented (upload/search/content/metadata).
- IN_PROGRESS: Missions endpoints still stubbed.

## Detailed API Parity

### HTTP/HTTPS TAK API (Marti) Endpoints

| Endpoint | Methods | Python Reference | fts-rs Status | Notes | Approval |
| --- | --- | --- | --- | --- | --- |
| / | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | Root handler | |
| /Alive | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | Health | |
| /Marti/vcm | GET, POST | `.../http_tak_api_service_main.py` | NOT_STARTED | Video connections | |
| /Marti/api/version/config | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | Version config | |
| /Marti/api/sync/metadata/<hash>/tool | PUT, GET | `.../http_tak_api_service_main.py` | NOT_STARTED | Metadata tool endpoints | |
| /Marti/api/version | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | Version | |
| /Marti/sync/missionquery | GET/POST/PUT/DELETE | `.../http_tak_api_service_main.py` | NOT_STARTED | Mission sync | |
| /Marti/api/citrap | GET | `.../blueprints/citrap_blueprint.py` | NOT_STARTED | CITRAP | |
| /Marti/api/groups/groupCacheEnabled | GET | `.../blueprints/misc_blueprint.py` | NOT_STARTED | Groups cache | |
| /Marti/api/clientEndPoints | GET | `.../blueprints/misc_blueprint.py` | NOT_STARTED | Client endpoints | |
| /Marti/sync/upload | POST | `.../blueprints/enterprise_sync_blueprint.py` | NOT_STARTED | Enterprise sync upload | |
| /Marti/sync/search | GET | `.../blueprints/enterprise_sync_blueprint.py` | NOT_STARTED | Enterprise sync search | |
| /Marti/sync/content | HEAD, PUT, POST, GET | `.../blueprints/enterprise_sync_blueprint.py` | NOT_STARTED | Enterprise content | |
| /Marti/sync/missionupload | PUT, POST | `.../blueprints/enterprise_sync_blueprint.py` | NOT_STARTED | Mission upload | |
| /Marti/sync/missionquery | GET | `.../blueprints/enterprise_sync_blueprint.py` | NOT_STARTED | Mission query | |
| /Marti/api/missions | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission list | |
| /Marti/api/missions/invitations | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission invitations | |
| /Marti/api/groups/all | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Groups list | |
| /Marti/api/missions/<mission_id> | PUT, GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission CRUD | |
| /Marti/api/missions/<mission_id>/cot | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission CoT | |
| /Marti/api/missions/<mission_id>/contents | PUT | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission contents | |
| /Marti/api/missions/logs/entries | POST, PUT | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission logs | |
| /Marti/api/missions/logs/entries/<id> | DELETE, GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission log entry | |
| /Marti/api/missions/<missionID>/log | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission log | |
| /Marti/api/missions/all/logs | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | All mission logs | |
| /Marti/api/missions/<child_mission_id>/parent/<parent_mission_id> | PUT | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Parent mission set | |
| /Marti/api/missions/<child_mission_id>/parent | DELETE, GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Parent mission | |
| /Marti/api/missions/<parent_mission_id>/children | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission children | |
| /Marti/api/missions/all/subscriptions | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | All subscriptions | |
| /Marti/api/missions/<mission_id>/subscriptions | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission subscriptions | |
| /Marti/api/missions/<mission_id>/subscription | PUT, DELETE, GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission subscription | |
| /Marti/api/missions/<mission_id>/subscriptions/roles | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Roles | |
| /Marti/api/missions/<mission_id>/externaldata | POST | `.../blueprints/mission_blueprint.py` | NOT_STARTED | External data | |
| /Marti/api/missions/<mission_id>/changes | GET | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission changes | |
| /Marti/api/missions/<mission_id>/contents/missionpackage | PUT | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission package | |
| /Marti/api/missions/<mission_id>/invite/<type>/<invitee> | PUT | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission invite (typed) | |
| /Marti/api/missions/<mission_id>/invite | POST | `.../blueprints/mission_blueprint.py` | NOT_STARTED | Mission invite | |
| /Marti/api/excheck/<checklistUid>/stop | POST | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck stop | |
| /Marti/api/excheck/<templateUid>/start | POST | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck start | |
| /Marti/api/excheck/checklist | POST | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck checklist | |
| /Marti/api/excheck/checklist/<checklistUid> | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck checklist | |
| /Marti/api/excheck/checklist/<checklistUid>/mission/<missionName> | PUT, DELETE | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck mission link | |
| /Marti/api/excheck/checklist/<checklistUid>/status | GET, DELETE | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck status | |
| /Marti/api/excheck/checklist/<checklistUid>/task/<taskUid> | GET, PUT, DELETE | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck task | |
| /Marti/api/excheck/checklist/active | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | Active checklists | |
| /Marti/api/excheck/template | POST | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck template | |
| /Marti/api/excheck/template/<templateUid> | GET, DELETE | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck template | |
| /Marti/api/excheck/template/<templateUid>/task/<taskUid> | GET, PUT, DELETE, POST | `.../http_tak_api_service_main.py` | NOT_STARTED | Template task | |
| /Marti/api/excheck/<subscription>/start | POST | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck subscription | |
| /Marti/api/missions/exchecktemplates/changes | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck templates changes | |
| /Marti/api/missions/ExCheckTemplates | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck templates | |
| /Marti/api/missions/exchecktemplates | GET | `.../http_tak_api_service_main.py` | NOT_STARTED | ExCheck templates | |

### REST API Endpoints

| Endpoint | Methods | Python Reference | fts-rs Status | Notes | Approval |
| --- | --- | --- | --- | --- | --- |
| /Alive | GET | `.../rest_api_service_main.py` | NOT_STARTED | Health | |
| /ManageSystemUser/getAll | GET | `.../rest_api_service_main.py` | NOT_STARTED | System users | |
| /ManageSystemUser/getSystemUser | GET | `.../rest_api_service_main.py` | NOT_STARTED | System user | |
| /ManageSystemUser/putSystemUser | PUT | `.../rest_api_service_main.py` | NOT_STARTED | Update system user | |
| /ManageSystemUser/postSystemUser | POST | `.../rest_api_service_main.py` | NOT_STARTED | Create system user | |
| /ManageSystemUser/deleteSystemUser | DELETE | `.../rest_api_service_main.py` | NOT_STARTED | Delete system user | |
| /ManageNotification/getNotification | GET | `.../rest_api_service_main.py` | NOT_STARTED | Notifications | |
| /SendGeoChat | POST | `.../rest_api_service_main.py` | NOT_STARTED | GeoChat | |
| /ManagePresence | GET | `.../rest_api_service_main.py` | NOT_STARTED | Presence base | |
| /ManagePresence/postPresence | POST | `.../rest_api_service_main.py` | NOT_STARTED | Presence create | |
| /ManagePresence/putPresence | PUT | `.../rest_api_service_main.py` | NOT_STARTED | Presence update | |
| /ManageRoute | GET | `.../rest_api_service_main.py` | NOT_STARTED | Route base | |
| /ManageRoute/postRoute | POST | `.../rest_api_service_main.py` | NOT_STARTED | Route create | |
| /ManageCoT/getZoneCoT | GET | `.../rest_api_service_main.py` | NOT_STARTED | Zone CoT | |
| /ManageGeoObject | GET | `.../rest_api_service_main.py` | NOT_STARTED | GeoObject base | |
| /ManageGeoObject/getGeoObject | GET | `.../rest_api_service_main.py` | NOT_STARTED | GeoObject get | |
| /ManageGeoObject/putGeoObject | PUT | `.../rest_api_service_main.py` | NOT_STARTED | GeoObject update | |
| /ManageVideoStream | GET | `.../rest_api_service_main.py` | NOT_STARTED | Video base | |
| /ManageVideoStream/getVideoStream | GET | `.../rest_api_service_main.py` | NOT_STARTED | Video get | |
| /ManageVideoStream/deleteVideoStream | DELETE | `.../rest_api_service_main.py` | NOT_STARTED | Video delete | |
| /ManageVideoStream/postVideoStream | POST | `.../rest_api_service_main.py` | NOT_STARTED | Video create | |
| /ManageChat | GET | `.../rest_api_service_main.py` | NOT_STARTED | Chat base | |
| /ManageChat/postChatToAll | POST | `.../rest_api_service_main.py` | NOT_STARTED | Chat send | |
| /Sensor | GET | `.../rest_api_service_main.py` | NOT_STARTED | Sensor base | |
| /Sensor/postDrone | POST | `.../rest_api_service_main.py` | NOT_STARTED | Drone post | |
| /Sensor/postSPI | POST | `.../rest_api_service_main.py` | NOT_STARTED | SPI post | |
| /MapVid | POST | `.../rest_api_service_main.py` | NOT_STARTED | Map video | |
| /AuthenticateUser | GET | `.../rest_api_service_main.py` | NOT_STARTED | Auth | |
| /APIUser | GET, POST, DELETE | `.../rest_api_service_main.py` | NOT_STARTED | API user | |
| /RecentCoT | GET | `.../rest_api_service_main.py` | NOT_STARTED | Recent CoT | |
| /URL | GET | `.../rest_api_service_main.py` | NOT_STARTED | URL | |
| /Clients | GET | `.../rest_api_service_main.py` | NOT_STARTED | Clients | |
| /FederationTable | GET, POST, PUT, DELETE | `.../rest_api_service_main.py` | NOT_STARTED | Federation table | |
| /ManageKML/postKML | POST | `.../rest_api_service_main.py` | NOT_STARTED | KML | |
| /BroadcastDataPackage | POST | `.../rest_api_service_main.py` | NOT_STARTED | Broadcast datapackage | |
| /checkStatus | GET | `.../rest_api_service_main.py` | NOT_STARTED | Status | |
| /manageAPI/getHelp | GET | `.../rest_api_service_main.py` | NOT_STARTED | Help | |
| /v2/<context>/<action> | GET, POST | `.../rest_api_service_main.py` | NOT_STARTED | v2 routing | |

## Milestones

1) Baseline server compiles + runs with TCP/TLS CoT and minimal HTTP server parity
2) Marti HTTP API parity (mission + enterprise sync + metadata + version)
3) REST API parity
4) ExCheck + mission extras + enterprise sync completeness
5) Federation client/server parity
6) Final hardening, docs, and acceptance tests

## Acceptance Tests (to be filled)
- [ ] CoT TCP connect + broadcast + persistence
- [ ] CoT TLS connect + broadcast + persistence
- [ ] Marti upload + metadata download + URL in payload
- [ ] Mission CRUD + subscription + logs
- [ ] ExCheck templates + checklist start/stop
- [ ] Federation sync between two servers
- [ ] REST API health, users, KML
- [ ] Data packages (client-to-client) end-to-end
- [ ] WinTAK + ATAK basic connectivity
