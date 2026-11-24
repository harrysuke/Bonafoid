# Bonafoid — Frequently Asked Questions

**For:** New and non-technical users.
**Companion documents:** [End-User Manual](user-manual.md) · [Troubleshooting Guide](troubleshooting.md).

Answers are grouped by what you are trying to do. All labels mentioned here are the ones you will actually see in the web IDE.

---

## Getting started and accounts

### What exactly is Bonafoid?
A tool that lets you build an application by drawing and configuring it instead of writing code. You design your data, forms, pages and workflows; Bonafoid writes the working application files for you. See the [manual](user-manual.md#1-what-is-bonafoid).

### Do I need to know how to program?
No. All the builders use buttons, forms and drag-and-drop. A few advanced panels accept JSON (a structured text format), but you can leave those untouched.

### How do I get access?
Open the Bonafoid address given to you by your administrator, choose **Create one** under **No account? Create one**, and register with an email and a password of at least 8 characters. On a local installation the address is usually `http://127.0.0.1:8081`.

### I forgot my password. What now?
There is no **forgot password** link — Bonafoid does not send emails, so a reset link cannot be delivered. Ask the person who runs your Bonafoid server to help you regain access, and keep your password safe in future.

### Why does it say "Invalid email or password"?
The email or password did not match. Check for typos and that Caps Lock is off. If you only just registered, try signing in again rather than creating a second account.

### Can two people share one account?
Technically yes, but it is not recommended. Projects are private to the account, and if two people edit the same project at once the later save wins. For teams, use **Workspaces** and give each person their own account.

---

## Projects and saving

### Where are my projects stored?
In the web IDE, on the server, tied to your account. You can see them all on the **Projects** screen. The desktop app instead opens one project at a time from your computer.

### How do I back up a project?
On the **Projects** screen, use **Export JSON** on a project row. That downloads a copy you can keep. To restore it, use **Import project** on the same screen. Importing always creates a new project — it never overwrites an existing one.

### Can I copy a project?
Yes. Use **Duplicate** on the project row. This is the safest way to try changes without risking your original.

### What does the `*` next to the project name mean?
You have unsaved changes. Wait a moment for autosave, or select **Save project** in the header, and the `*` will disappear.

### Do I have to save manually?
No. Autosave runs about 1.5 seconds after you stop editing. **Save project** just does it immediately. If you see **Autosave failed**, do not close the tab — try **Save project** again and check your connection.

### Why was I asked "This project has unsaved changes. Leave without saving them?"
You tried to leave with edits that had not been saved yet — by closing the tab, going to **Projects**, signing out or opening another project. Choose **Cancel** to stay, or **OK** to leave and discard the unsaved edits.

### Can I undo changes?
There is no simple undo button for the whole editor. For version history, use the **Version** screen (commit, browse history, compare with **Diff**, and **Rollback**). Until you commit, keep a **Duplicate** of important projects.

---

## Building your app

### What is the difference between Forms, Data Model, Pages and Lists?
- **Data Model** defines the information your app stores (like spreadsheet columns).
- **Forms** collect information from people.
- **Pages** are the screens people see.
- **Lists** quickly create a page that shows stored records in a table or list.

A typical order is: Data Model → Forms → Lists/Pages → Theme → Preview → Generate.

### Why doesn't my form actually submit anything in Preview mode?
The editor only *designs* the form. Submissions are handled by the generated application once you **Generate** and run it. The preview panel even says: *"Preview only. Submissions are handled by the generated application, not by the IDE."*

### Where did the drag-and-drop data canvas go?
This version edits data in a table on the **Data Model** screen, not by dragging boxes onto a canvas. Create an entity, then add fields and relations in the tables provided.

### How do I add a field to a form?
Open the form, click a field type in the **Field Types** panel on the left (for example **Text Input**), then click the new field on the canvas and set its **Label**, **Name**, **Placeholder** and **Required** in **Field Properties** on the right.

### What does "Required" mean?
People must fill the field in before the form can be submitted. **Unique** means no two records may share the same value.

### My page component looks empty. Why?
Data components (tables, charts, detail views) usually need a **Data binding** in the inspector — a **Data source**, **Path** and **Prop**. Items in the palette marked **bind** need this to show live data.

### What are "page actions"?
Things that happen when someone interacts with your page — for example **Navigate**, **Submit form**, **Call API**, **Open modal** or **Download**. Add them in the **Page actions** section of the inspector.

### Can I change my app's colours and fonts?
Yes. Open **Theme**, adjust colours under **Colors** and fonts under **Typography**, watch the **Live preview**, then select **Set active**. Only the active theme affects the generated app.

### Why is the Framework "Leptos" and the Bundler greyed out?
Those are the only options in this version. Bonafoid generates a Leptos frontend compiled with Trunk. The disabled control is intentional, not a broken setting.

---

## Workflows

### What is a workflow?
A drawing of a process: where it starts, what happens, which decisions are made, and where it ends. Bonafoid uses BPMN 2.0, a standard format for process drawings.

### How do I know my workflow is correct?
Select **Validate** in the toolbar. Bonafoid reports **Valid: N node(s), N flow(s).** or **Invalid: N issue(s).** Fix the reported issues and validate again.

### Why doesn't my published app do anything with my workflow?
In this version, workflows are a design and testing artifact. The IDE can validate and run them, but the generated application does not execute them at runtime. This is a known limitation, not a setup mistake.

### Can I use a workflow someone else made?
Yes. Use **Import BPMN**, give it a **Process name**, and paste or load the BPMN XML. Unsupported elements may be skipped with a warning. Import creates a new process; it does not overwrite an existing one.

---

## Preview and generating code

### What is the difference between Preview and Generate?
- **Preview** builds and runs your app on the server so you can try it inside Bonafoid.
- **Generate** writes the actual application files and, in the web IDE, downloads them as a `.zip` that you can keep and deploy.

### Why does Preview say "Hosted preview unavailable"?
The server does not have the build tools installed. Use **Generate** to get the source files and build them elsewhere, or ask your administrator to install the tools. See the [troubleshooting guide](troubleshooting.md#preview-says-hosted-preview-unavailable).

### Preview stopped working after a while. Why?
Previews are stopped automatically after a period of inactivity to free up resources. Select **Build & Run** (or **Rebuild**) and **Start app** again.

### Can I limit what gets generated (only the frontend, for example)?
No. Code generation always writes the backend, frontend, database, Docker and Kubernetes files. There are no checkboxes to choose subsets.

### Which database should I choose?
Choose **SQLite**. It needs no database server and is the only option fully supported end to end. Other choices generate files but are not verified; previews against them are not supported without extra server configuration.

### In what order should I do things?
Design → **Preview** (check it works) → **Generate** (get the files) → **Deploy** (publish). See the [tutorial](user-manual.md#5-tutorial-build-your-first-app).

---

## Plugins, deployment and team features

### Where is the plugin marketplace?
There is no marketplace in this version. You install a plugin by supplying two files you already have: a **Manifest (plugin.json)** and a **Module (.wasm)**. Use the **Plugins** screen.

### Can plugins see my other projects?
Plugins are private to your account and run in a sandbox with a limited budget and memory cap. They still request permissions (shown on each plugin card), so only install plugins you trust.

### Why does the Plugins screen say "Plugins need the IDE server"?
You are using the desktop IDE, which has no plugin sandbox. Use the web IDE to install plugins.

### What does "Deployed" mean, and why does my deploy show "Pushed — CI pending"?
Publishing has two stages: Bonafoid pushes your code, then an automated build runs. A push alone is not a deployment. The status becomes **Deployed** only after that automated build succeeds for your exact version. Use **Refresh history** to check again later.

### Why does Deploy say it is not available on this host?
The server has not been configured with the deployment secret. Ask your administrator to set it up. It is not a problem with your project.

### I changed a setting and the generated app did not change. Why?
Only the three tabs on **Settings** — **General**, **Database** and **Frontend** — affect generated code. Authentication, email, API, storage and notification tabs were removed because changing them had no effect.

### Where did my other Settings tabs go?
They were removed on purpose. The values still exist inside project files so old projects import without data loss, but since nothing in code generation read them, showing them would have been misleading.

---

## Still stuck?

Work through the [Self-Service Troubleshooting Guide](troubleshooting.md), which is organised by the exact message you may see on screen. If nothing there helps, contact your administrator and include the message text and what you were doing when it appeared.
