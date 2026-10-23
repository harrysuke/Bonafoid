# Bonafoid User Manual

## Build a CRUD application, from idea to deployment

This manual walks through building a real CRUD application with Bonafoid,
from signing in to running the result on your own machine. Every button
name, field label and URL in it is the real one, taken from the shipping
build.

**What you will build:** a *Product catalogue* — one entity (`Product`)
with a form, a list page, and a deployed backend you can call with `curl`.

**Time:** about an hour, most of it waiting for the first build.

---

## Table of contents

1. [Before you start](#1-before-you-start)
2. [The worked example](#2-the-worked-example)
3. [Sign in and create a project](#3-sign-in-and-create-a-project)
4. [Define the data model](#4-define-the-data-model)
5. [Define the form](#5-define-the-form)
6. [Generate the list page](#6-generate-the-list-page)
7. [Design the pages](#7-design-the-pages)
8. [Project settings](#8-project-settings)
9. [Generate the code](#9-generate-the-code)
10. [Run it](#10-run-it)
11. [Verify the CRUD](#11-verify-the-crud)
- [Appendix A — The generated API](#appendix-a--the-generated-api)
- [Appendix B — Troubleshooting](#appendix-b--troubleshooting)
- [Appendix C — IDE screen map](#appendix-c--ide-screen-map)

---

## 1. Before you start

### How Bonafoid works

Bonafoid turns a **visual model** into a **real application**:

```
   model  ──▶  code  ──▶  running app
 (the IDE)   (Rust +    (Axum API + Leptos
             Leptos)     frontend)
```

You never write the API by hand. You describe *entities* (tables),
*forms* (input screens), *pages* (screens) and *workflows* (processes), and
the generator writes the backend, the frontend, the SQL schema, the Docker
files and the Kubernetes manifests.

### The two hosts

| Host | What it is | When to use it |
| --- | --- | --- |
| **Web IDE** | React app served by the IDE server at `http://127.0.0.1:8081` | Shared server, multiple people, projects stored server-side |
| **Desktop IDE** | Tauri v2 app on your own machine | Local files, no server needed |

This manual uses the **web IDE**. Everything in it applies to the desktop
app too, except the sign-in and project-picker steps (chapter 3) and the
download in chapter 9.

### What you need

| Requirement | Why | How to check |
| --- | --- | --- |
| Rust (stable) + Cargo | The IDE server, and building the generated app | `cargo --version` |
| Node.js 20+ and npm | The IDE's own frontend | `node --version` |
| `wasm32-unknown-unknown` target | The generated frontend is Rust→WASM | `rustup target add wasm32-unknown-unknown` |
| [Trunk](https://trunkrs.dev) | Builds the generated frontend | `cargo install trunk` |
| Docker + Compose | Only for chapter 10's container path | `docker --version` |

The IDE server also needs `cargo` and `trunk` on the machine that runs it,
because the hosted preview builds the real project.

### Start the IDE server

```powershell
npm ci
npm run build:web
cargo run -p nocode-ide-server
```

Then open **<http://127.0.0.1:8081>**.

---

## 2. The worked example

We will build a **Product catalogue**.

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `id` | `UUID` | yes | Created for you — the primary key |
| `name` | `String` | yes | The product name |
| `price` | `Decimal` | yes | Two decimal places |
| `description` | `Text` | no | Long description |
| `in_stock` | `Boolean` | yes | Default `true` |

The **API** the generator will produce:

```
GET    /api/v1/product          list
POST   /api/v1/product          create
GET    /api/v1/product/:id      read
PUT    /api/v1/product/:id      update
DELETE /api/v1/product/:id      delete
```

Everything below builds exactly that.

---

## 3. Sign in and create a project

The web IDE keeps projects server-side, so it starts with an account.

1. Open **<http://127.0.0.1:8081>**. You are sent to **/login**.
2. No account yet? Choose **Create one** under the form, which takes you to
   **/register**.
3. Fill in **Email**, **Password** and **Confirm password**, then press
   **Create account**. Passwords must be at least 8 characters.
4. Back on **/login**, enter your **Email** and **Password** and press
   **Sign in**.

You land on the **Projects** screen — a list of the projects stored for
your account.

5. In the **New project name** box, type `Product Catalogue` and press
   **Create**.
6. Press **Open** on the new project.

The IDE loads. The top bar shows the project name on the right, and the
left sidebar shows the eleven screens you can work in.

> **The desktop IDE** has no sign-in. Use **New project** in the top bar
> instead, and the project lives in a file on your machine.

---

## 4. Define the data model

This is where you describe your tables. Choose **Data Model** in the
sidebar (route `/data`).

### 4.1 Create the entity

1. Press **Add Entity** (top right).
2. An inline form appears in the left panel with the placeholder
   **Entity name (PascalCase)**. Type `Product`.
3. Press **Create**.

Two things happen automatically:

- The entity is named `Product` and its plural is `Products`.
- **A primary key is created for you**: a field `id` of type `UUID`, marked
  `PK`, `NOT NULL` and `UNIQUE`. You do not add it yourself.

The left panel now shows **Entities (1)** and the centre panel shows the
entity's fields.

### 4.2 Add the fields

Press **Add Field** in the centre panel for each field. A new field is
called `field_2`, `field_3` and so on, of type `String` and not required.

For each one, edit the **Name** box in the **Fields** table and pick a
**Type** from the dropdown.

| Field | Type | Required | Unique | Default |
| --- | --- | --- | --- | --- |
| `name` | `String` | tick | – | – |
| `price` | `Decimal` | tick | – | – |
| `description` | `Text` | leave unticked | – | – |
| `in_stock` | `Boolean` | tick | – | `true` |

The **Type** dropdown offers:

```
String · Text · Integer · Big Integer · Float · Decimal · Boolean
Date · DateTime · JSON · UUID · Enum · Relation
```

Tick **Required** to make a field mandatory, tick **Unique** to prevent
duplicates, and type into **Default** for a starting value.

> The **Default** value is parsed as JSON, falling back to plain text. So
> `true`, `42` and `"hello"` all work.

### 4.3 Read the Schema panel

The right-hand **Schema** panel shows what the generator will create,
with a **Flags** column: `PK`, `NOT NULL`, `UNIQUE`.

It also says this, which is worth reading:

> *DDL and migrations are emitted by the code generator, which maps each
> field type onto the project's database driver. Run Generate Code to
> produce them.*

### 4.4 (Optional) Relations

If one entity points at another, press **Add Relation**. The **Type**
dropdown offers `Belongs To`, `Has One`, `Has Many` and `Many to Many`.

Relations become real foreign keys in the generated SQL.

> **Many to Many is not implemented yet.** The generator skips it, so no
> join table is created. Use `Belongs To` and hold the other side of the
> relationship in your own code.

---

## 5. Define the form

A form is a typed input screen. Choose **Forms** in the sidebar (route
`/forms`).

1. Press **Create Form**. A form named `Untitled Form` is created.
2. Rename it in the **Form name** box at the top-left. Call it
   `ProductForm`.
3. The **Field Types** panel on the left lists 20 field types. Click one to
   add it to the form:

```
📝 Text Input  📄 Text Area  📧 Email  🔗 URL  📞 Phone
🔢 Number  🔢 Integer  ✅ Boolean  📋 Dropdown  🔘 Radio Group
☑️ Checkbox Group  📅 Date  🕐 Date & Time  ⏰ Time
📎 File Upload  🖼️ Image  ✍️ Signature  📰 Rich Text
🧾 JSON  🔗 Relation
```

4. Add four fields, and give each a **Label** and a **Name** in the
   **Field Properties** panel:

   | Palette tile | Label | Name |
   | --- | --- | --- |
   | 📝 Text Input | Name | `name` |
   | 🔢 Number | Price | `price` |
   | 📄 Text Area | Description | `description` |
   | ✅ Boolean | In stock | `in_stock` |

   The palette has no Decimal tile, so `price` uses **Number** here. If
   you want decimals in the database, set the field's type to `Decimal`
   in the **Data Modeler** (chapter 4) and give it the same name.
5. Click a field to select it. The **Field Properties** panel on the right
   has these controls, in order:

| Control | Meaning |
| --- | --- |
| **Label** | What the user sees. May be empty — the field then shows as `Untitled` |
| **Name** | The field's name in the submitted data |
| **Placeholder** | Hint text inside the input |
| **Required** | Tick to make it mandatory |
| **Options** | Only for Dropdown, Radio Group and Checkbox Group. One box per option, plus **Add Option** |
| **Delete** | Removes the field |

6. Press **Preview** in the toolbar to see the form as a user will. Press
   **Edit** to go back.

> The preview is a preview. Submissions are handled by the generated
> application, not by the IDE.

---

## 6. Generate the list page

This turns your entity into a ready-made page. Choose **Lists** in the
sidebar (route `/lists`).

1. In the **Entity** dropdown, pick `Product`.
2. In the **Component kind** dropdown, pick `Table` (or `List`).
3. Press **Generate Page**.

A page named `Products List` is created at the path `/products/list`, with
a `Product Data` data source and five page actions:

| Action | Method | What it does |
| --- | --- | --- |
| `Create Record` | `POST` | Creates a row |
| `Read Record` | `GET` | Reads one row |
| `Update Record` | `PUT` | Updates a row |
| `Delete Record` | `DELETE` | Deletes a row |
| `Reload Rows` | `GET` | Refreshes the list |

The **Preview** panel shows the JSON of the page that will be generated,
before you generate it.

> **Read this before you rely on the list page.** The generated *list* page
> is a shell: its table renders a placeholder (`(data from Product Data)`)
> because data-bound components are not yet wired to a live query. Its five
> actions are real and call the right endpoints, but nothing on the page
> triggers them yet.
>
> **The working CRUD screen is the entity page**, which the generator
> creates for you automatically — see chapter 10. It has a live table, a
> create form and a delete button, all wired to the API.
>
> The List Builder is still worth using: it creates the page, the data
> source and the actions, and it is what you will bind to buttons once
> data-bound components land.

---

## 7. Design the pages

Choose **Pages** in the sidebar (route `/pages`) for full control over
your own screens.

1. Press **New Page**, type `Product Catalogue` in the **Page name** box
   and press **Create**.
2. The path is derived from the name (`/product-catalogue`). Edit **Path**
   in the left panel if you want something else.
3. The **Palette** on the right is grouped into six categories:

| Category | Components |
| --- | --- |
| **LAYOUT** | Container, Grid, Flex, Card, Tabs, Accordion, Divider |
| **DATA** | Table, Detail view, Chart, Stat card, Progress bar, Tree, Transfer, Cascader, Tree select |
| **BASIC** | Text / HTML, Image, Avatar, Badge, Tag, Video, Audio, Iframe, Custom |
| **INPUT** | Button, Button group, Form, Form field, Date picker, Time picker, Color picker, File upload, Rate, Slider, Mention |
| **FEEDBACK** | Alert, Message, Notification, Loading, Skeleton, Spin, QR code |
| **NAVIGATION** | Link, Breadcrumb, Steps, Menu, Anchor, Back to top, Pagination |

Click a component to add it to the selected page. Components marked
**bind** need a data source, a form or an action.

4. **Data bindings** connect a component to a data source. In the selected
   component's panel, choose a **Data source**, fill in **Path** and
   **Prop**, then press **Add binding**.
   * **Data source** and **Prop** are both required; **Path** is optional.
5. **Page actions** are reusable operations. Choose an **Action** and press
   **Add**. The options are:

```
Navigate · Open modal · Close modal · Submit form · Reset form
Call API · Execute workflow · Set state · Emit event
Download · Copy to clipboard · Print
```

6. Use the icons on each component card to move it up or down, duplicate
   it, or delete it.

---

## 8. Project settings

Choose **Settings** in the sidebar (route `/settings`). These values are
written into the generated application.

There are three tabs.

### General

| Field | Notes |
| --- | --- |
| **Project Name** | Renaming also rewrites the slug, which the generated crate name comes from |
| **Slug** | Read-only. The crate, the binary and the Docker image are named after it |
| **Description** | Free text |

### Database

| Field | Notes |
| --- | --- |
| **Database Driver** | `SQLite`, `PostgreSQL`, `MySQL`, `SQL Server`, `Oracle` |
| **Connection URL** | Placeholder changes with the driver: `sqlite://app.db` or `postgres://user@host/db` |

> **SQLite is the default.** A new project needs no database server: the
> generated backend creates its own SQLite file, and the hosted preview
> gives it a throwaway one. Only **PostgreSQL** and **SQLite** are
> validated end to end.
>
> **SQL Server and Oracle cannot be generated.** SeaORM 0.12 ships only
> the SQLite, MySQL and Postgres drivers, so the generator refuses those
> two rather than quietly producing a Postgres backend under another
> name. Pick one of the three that work.
>
> The connection URL is compiled into the generated backend. **Previews
> never receive it** — a preview runs generated code, so it uses a
> throwaway database instead.

### Frontend

| Field | Notes |
| --- | --- |
| **Framework** | `Leptos (Rust / WASM)` — the only one |
| **Bundler** | `Trunk`. Read-only: Trunk is the supported build tool |

Press **Save Changes** when you are done.

---

## 9. Generate the code

Choose **Generate** in the sidebar (route `/generate`), or press the
**Generate Code** button at the bottom of the sidebar.

1. Set **Output Directory**. It defaults to `./generated`.
2. Choose the **Database** driver.
3. The **What will be generated** panel lists the counts: `Data models`,
   `Forms`, `Pages`, `Workflows`, `Themes`.
4. Press **Generate Code**.

The **Output** panel shows the status (`Idle` → `Generating...` →
`Ready`), a count of **Backend Files**, **Frontend Files** and
**Migrations**, and a **Generation Log**.

### On the web

The server owns the workspace, so the **Output Directory** box is ignored —
the output is written server-side. When it finishes, the IDE downloads the
project as a ZIP named `<project-id>.zip`.

### On the desktop

The files are written to the directory you chose.

### What you get

```
<output>/
├── README.md            the generated project's own quick start
├── Makefile             dev / build / test / docker-up / k8s-deploy
├── .dockerignore
├── backend/             Rust + Axum API
│   ├── Cargo.toml
│   ├── migrations/0001_initial.sql
│   └── src/{main,config,db,error,routes}.rs, models/, handlers/
├── frontend/            Leptos + WASM
│   ├── Cargo.toml, Trunk.toml, index.html
│   ├── public/          manifest.webmanifest, sw.js, offline.html, icons/
│   └── src/{api,app,types,convert,main}.rs, pages/, components/
├── database/            schema.sql, migrations/0001_up.sql, 0001_down.sql
├── desktop/             Tauri shell
├── docker/              Dockerfile, Dockerfile.frontend, docker-compose.yml
└── kubernetes/          deployment, service, ingress, configmap, secret
```

---

## 10. Run it

There are three ways to get a running app. Pick one.

### 10.1 The hosted preview — the fastest route

The IDE server builds and runs the project for you. No ZIP, no Docker.

1. Choose **Preview** in the sidebar (route `/preview`).
2. Set **Target** to `App + API`.
3. Press **Build & Run**.

The panel shows the build state as it goes: `Queued` → `Generating code`
→ `Building backend` → `Building frontend` → `Ready`.

The **Build log** on the right streams the compiler output.

4. When the state reads **Ready**, the app starts by itself and appears in
   the iframe on the left.
5. Press **Open preview in a new tab** to use it like a normal website.

The generated app's URLs are:

```
http://127.0.0.1:8081/preview/<build-id>/          the frontend
http://127.0.0.1:8081/preview/<build-id>/api/...   the API
```

The proxy forwards `/preview/<build-id>/api/*` to the running backend
*including* the `api` segment, so the backend sees `/api/v1/product`.

Things worth knowing:

- **The preview is reaped after 15 minutes of no traffic.** Raise
  `IDE_PREVIEW_IDLE_SECS` if you want it to stay up longer.
- **A SQLite preview gets a throwaway database** inside the build
  workspace (`sqlite://preview.db?mode=rwc`). Your rows are not in your
  project database.
- **A non-SQLite preview needs `IDE_PREVIEW_DATABASE_URL`** to be set on
  the server, and the server refuses to point it at the project's own
  database.
- **The frontend's `API_BASE` is baked in at build time**, pointing at this
  build's own preview URL. That is why the bundle works without any
  configuration.
- Press **Cancel** to stop a build, **Stop app** to stop a running app.

### 10.2 Run it on your own machine

Unzip the download (or use the directory you chose on the desktop), then:

**Backend**

```powershell
cd backend
$env:DATABASE_URL="sqlite://app.db?mode=rwc"
cargo run
```

It listens on **<http://127.0.0.1:3000>** and creates the schema on
startup.

**Frontend** (in a second terminal)

```powershell
cd frontend
trunk serve
```

It serves on **<http://localhost:8080>**.

> The frontend calls the backend at `API_BASE`, which is **baked into the
> bundle when it is compiled**. The default is `http://127.0.0.1:3000`,
> which matches the backend's default port. To point it somewhere else,
> set it *before* building:
>
> ```powershell
> $env:API_BASE="http://127.0.0.1:3000"
> trunk serve
> ```
>
> No trailing slash, please — the client appends the path directly.

### 10.3 Docker Compose

Run this from the **project root**, not from `docker/`:

```bash
docker compose -f docker/docker-compose.yml up -d
```

This starts the services your project's database needs. With the default
**SQLite** project there are two:

| Service | Port | What it is |
| --- | --- | --- |
| `backend` | 3000 | The API |
| `frontend` | 8080 | The built site |

A **PostgreSQL** project adds a `postgres` service on 5432, and a
**MySQL** project adds a `mysql` service on 3306. The Compose file is
generated from the project's driver, so it always matches.

Open **<http://localhost:8080>**.

The backend creates the schema on startup, so there is nothing else to run
first. The frontend image is built with `API_BASE=http://localhost:3000`,
so the browser talks to the published backend port.

To stop everything:

```bash
docker compose -f docker/docker-compose.yml down
```

> **Why the project root?** Relative paths in a Compose file are resolved
> against the Compose file's own directory. Running
> `docker-compose up` from inside `docker/` works too, but the file
> expects `../frontend` and `..` as the build context.

#### Building the images by hand

```bash
# The API
docker build -t my-app -f docker/Dockerfile .
docker run -p 3000:3000 -e DATABASE_URL=sqlite://app.db?mode=rwc my-app

# The frontend. API_BASE is a build argument, because it is baked in.
docker build -t my-app-frontend -f docker/Dockerfile.frontend \
  --build-arg API_BASE=http://127.0.0.1:3000 .
docker run -p 8080:80 my-app-frontend
```

### 10.4 Kubernetes

```bash
docker build -t my-app:latest -f docker/Dockerfile .
docker build -t my-app-frontend:latest -f docker/Dockerfile.frontend \
  --build-arg API_BASE=https://my-app.example.com .
docker push my-app:latest
docker push my-app-frontend:latest

kubectl apply -f kubernetes/
```

Before the Ingress becomes ready you need a TLS secret for the host it
names:

```bash
kubectl create secret tls my-app-tls --cert=tls.crt --key=tls.key
```

Two things to change first:

- `kubernetes/secret.yaml` ships a placeholder database password.
- The Ingress host and the `API_BASE` the frontend image was built with
  must match.

---

## 11. Verify the CRUD

Point a browser or `curl` at the running backend. For the hosted preview
that is `http://127.0.0.1:8081/preview/<build-id>`; for a local run it is
`http://127.0.0.1:3000`.

### The frontend screen

Open the entity page — `/product` in the preview, because the route is the
**singular** entity name. It shows:

- a table of rows,
- a **Create** form with an input per editable field,
- a **Refresh** button,
- a **Delete** button on every row.

### The API, directly

```bash
# List (expect 200 and [])
curl http://127.0.0.1:3000/api/v1/product

# Create (expect 201 and the new row)
curl -X POST http://127.0.0.1:3000/api/v1/product \
  -H 'content-type: application/json' \
  -d '{"name":"Desk lamp","price":49.99,"in_stock":true}'

# Read one (expect 200)
curl http://127.0.0.1:3000/api/v1/product/<id>

# Update (expect 200)
curl -X PUT http://127.0.0.1:3000/api/v1/product/<id> \
  -H 'content-type: application/json' \
  -d '{"name":"Desk lamp","price":39.99,"in_stock":true}'

# Delete (expect 204)
curl -X DELETE http://127.0.0.1:3000/api/v1/product/<id>

# Health
curl http://127.0.0.1:3000/health     # "OK"
curl http://127.0.0.1:3000/ready      # "READY"
```

### The form endpoint

```bash
curl -X POST http://127.0.0.1:3000/api/v1/forms/product_form \
  -H 'content-type: application/json' \
  -d '{"name":"Desk lamp","price":49.99,"in_stock":true}'
```

> The form endpoint **validates and echoes** the payload with `201
> Created`, but it does not store anything: forms have no table in the
> generated schema yet. Use the entity API for persistence.

---

## Appendix A — The generated API

Every project gets the same API shape.

### Routes

| Method | Path | Success |
| --- | --- | --- |
| `GET` | `/health` | `200` — plain text `OK` |
| `GET` | `/ready` | `200` — plain text `READY` |
| `GET` | `/api/v1/{entity}` | `200` — JSON array |
| `POST` | `/api/v1/{entity}` | `201` — the created row |
| `GET` | `/api/v1/{entity}/:id` | `200` — one row |
| `PUT` | `/api/v1/{entity}/:id` | `200` — the updated row |
| `DELETE` | `/api/v1/{entity}/:id` | `204` — no content |
| `POST` | `/api/v1/forms/{form}` | `201` — the echoed payload |

`{entity}` is the **singular** entity name in snake case (`Product` →
`product`); `{form}` is the form name in snake case (`ProductForm` →
`product_form`).

There is no `PATCH`, and no `/api/v1/version`.

### Environment variables

The generated backend reads these. Put them in `backend/.env` or export
them before running.

| Variable | Required | Default | Meaning |
| --- | --- | --- | --- |
| `DATABASE_URL` | no | the project's URL | Connection string |
| `HOST` | no | `127.0.0.1` | Bind address |
| `PORT` | no | `3000` | Listen port |
| `APP_NAME` | no | project name | Startup log line |
| `RUST_LOG` | no | `info` | `tracing` filter |

The backend does **not** read `JWT_SECRET`, `FRONTEND_URL` or `REDIS_URL`,
even though older generated files mention them.

### Database URLs

| Driver | Format | |
| --- | --- | --- |
| SQLite | `sqlite://app.db?mode=rwc` | the default |
| PostgreSQL | `postgres://user:pass@host:5432/db` | |
| MySQL | `mysql://user:pass@host:3306/db` | |

SQL Server and Oracle have no entry because SeaORM 0.12 ships no driver
for either — the generator refuses them.

`?mode=rwc` matters: without it, SQLite will not create a missing file.
A relative SQLite path is relative to the **backend directory**, so run
the backend from there.

### The frontend's `API_BASE`

Compile-time only, via `option_env!("API_BASE")`. The default is
`http://127.0.0.1:3000`. No trailing slash. Changing it means rebuilding
the bundle.

---

## Appendix B — Troubleshooting

### The IDE

| Symptom | Cause | Fix |
| --- | --- | --- |
| Pressing **Create** on an entity does nothing | The name was empty | Entities and pages need a name; the button is a no-op without one |
| The page is not in the sidebar | It is on another route | The sidebar has eleven entries; a page is a record inside a builder, not a sidebar entry |
| The preview never leaves `Building frontend` | `trunk` is missing on the server | The amber strip says `cargo ok, trunk missing`. Install it: `cargo install trunk` |
| The preview says `Host toolchain: cargo missing` | `cargo` is not on the server's PATH | Start the server from a shell where cargo works |
| The preview's rows are empty | A SQLite preview uses a throwaway database | Your data is not in the project database. Set `IDE_PREVIEW_DATABASE_URL` for a non-SQLite driver |
| The preview disappeared after a while | The idle reaper stopped it | Set `IDE_PREVIEW_IDLE_SECS` higher than the default 900 |
| The generated app's API calls fail in the browser | `API_BASE` is wrong, or the backend is not running | Check the browser console for the URL it called, then check chapter 10.2 |

### The generated app

| Symptom | Cause | Fix |
| --- | --- | --- |
| The backend exits at startup | The database is unreachable | Check `DATABASE_URL`. The backend connects and applies the schema *before* it binds the port |
| `SQLite` says the file cannot be created | `mode=rwc` is missing | Use `sqlite://app.db?mode=rwc` |
| The frontend loads but the table is empty | The list page's table is a placeholder | Use the entity page (`/product`), which is wired |
| The frontend calls `http://127.0.0.1:3000/api/...` in production | `API_BASE` was baked in at build time | Rebuild the frontend with the right `API_BASE` |
| `docker compose` fails immediately | It was run from inside `docker/` with the wrong path | Run `docker compose -f docker/docker-compose.yml up -d` from the project root |
| The Docker build fails on `backend/Cargo.lock` | An old Dockerfile | The generator no longer copies a lockfile. Regenerate the project |
| The container is reported unhealthy | The image had no `curl` for its health check | The generator now installs `curl` in the runtime stage. Regenerate the project |
| The second boot fails with "constraint already exists" | Non-idempotent foreign keys | The generator now emits inline `REFERENCES`, which is idempotent. Regenerate the project |
| The Kubernetes frontend pods never start | The ConfigMap was never created | The generator now uses a real frontend image. Build and push it, then apply the manifests |

### Windows

Cargo on Windows intermittently fails with *"The process cannot access the
file because it is being used by another process"* (`os error 32`). It is a
host property, not a defect: the search indexer and antivirus hold files
that Cargo is renaming. Retry the build, or set `CARGO_BUILD_JOBS=1`.

---

## Appendix C — IDE screen map

| Screen | Route | What it is for | What it produces |
| --- | --- | --- | --- |
| **Dashboard** | `/` | Overview and quick actions | Nothing |
| **Forms** | `/forms` | Typed input screens | A form per screen |
| **Workflows** | `/workflows` | BPMN 2.0 process design | A workflow definition |
| **Data Model** | `/data` | Entities, fields and relations | The database schema |
| **Lists** | `/lists` | A page per entity | A list page with CRUD actions |
| **Pages** | `/pages` | Full control over screens | Pages, components, bindings, actions |
| **Theme** | `/theme` | Colours, fonts and spacing | The design token tree |
| **Plugins** | `/plugins` | Installed WebAssembly plugins | Local-manifest installs only |
| **Preview** | `/preview` | Build and run the project | A running app on the server |
| **Generate** | `/generate` | Write the code out | The whole project tree |
| **Settings** | `/settings` | Name, database and framework | Values compiled into the app |

### The top bar

| Control | Action |
| --- | --- |
| **New project** | Asks for a name, then creates one |
| **Open project** | Picks a project |
| **Save project** | Saves the current project |
| Theme toggle | Light or dark |
| Avatar | Settings and Logout |

The project name is shown with a ` *` when there are unsaved changes.

---

*Bonafoid — generated applications you can actually run.*
