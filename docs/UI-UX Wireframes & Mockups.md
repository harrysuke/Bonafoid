# UI/UX Wireframes & Mockups

## No-Code Visual Application Development Platform

**Version:** 1.3  
**Date:** October 2026  
**Status:** Living document — wireframes re-drawn against the shipped screens
(Phase 6 amendments: sidebar navigation, 3-tab settings, file-based plugin
install, no marketplace, no activity timeline)  

---

## 1. Design Principles & Style Guide

### 1.1 Design Principles
- **Clarity**: Clean, intuitive interface with minimal cognitive load
- **Productivity**: Optimized for rapid prototyping and development
- **Consistency**: Unified design system across all builders and components
- **Accessibility**: WCAG 2.1 AA compliant
- **Responsiveness**: Adaptive layouts for desktop IDE workflow
- **Dark/Light Mode**: Full theme support with user preference

### 1.2 Color Palette

| Name | Light Mode | Dark Mode | Usage |
|------|------------|-----------|-------|
| Primary | `#2563eb` (Blue-600) | `#3b82f6` (Blue-500) | Buttons, links, active states |
| Secondary | `#64748b` (Slate-500) | `#94a3b8` (Slate-400) | Secondary actions |
| Success | `#10b981` (Emerald-500) | `#34d399` (Emerald-400) | Success states |
| Warning | `#f59e0b` (Amber-500) | `#fbbf24` (Amber-400) | Warning states |
| Error | `#ef4444` (Red-500) | `#f87171` (Red-400) | Error states |
| Background | `#ffffff` (White) | `#0f172a` (Slate-900) | Main background |
| Surface | `#f8fafc` (Slate-50) | `#1e293b` (Slate-800) | Cards, panels |
| Border | `#e2e8f0` (Slate-200) | `#334155` (Slate-700) | Borders, dividers |

### 1.3 Typography

| Element | Font | Size | Weight | Line Height |
|---------|------|------|--------|-------------|
| Display | Inter / Geist | 48-60px | 700 | 1.2 |
| H1 | Inter / Geist | 32-36px | 600 | 1.25 |
| H2 | Inter / Geist | 24-28px | 600 | 1.3 |
| H3 | Inter / Geist | 18-20px | 600 | 1.35 |
| Body | Inter / Geist | 14-16px | 400-500 | 1.6 |
| Small | Inter / Geist | 12-13px | 400 | 1.5 |
| Code | JetBrains Mono | 12-14px | 400 | 1.5 |

### 1.4 Spacing System (8px Grid)
| Token | Value | Usage |
|-------|-------|-------|
| xs | 4px | Tight spacing |
| sm | 8px | Small gaps |
| md | 16px | Default spacing |
| lg | 24px | Section spacing |
| xl | 32px | Large sections |
| 2xl | 48px | Major sections |
| 3xl | 64px | Page sections |

---

## 2. Layout & Navigation

### 2.1 Main Application Shell

The shipped shell is a collapsible **left sidebar** with eleven entries, not a
horizontal nine-tab topbar. The topbar carries the project name, the save
status and the theme toggle; the sidebar carries navigation and the user menu.

```text
┌─────────────────────────────────────────────────────────────────────────┐
│  [Logo] No-Code IDE                        [Save] [Preview] [☾] [User ▾]│
├──────────────────┬──────────────────────────────────────────────────────┤
│ ▸ Dashboard      │  Main Canvas / Editor Area                           │
│ ▸ Forms          │  [Toolbar] [Content] [Bottom Panel]                  │
│ ▸ Workflows      │                                                      │
│ ▸ Data Model     │                                                      │
│ ▸ Lists          │                                                      │
│ ▸ Pages          │                                                      │
│ ▸ Theme          │                                                      │
│ ▸ Plugins        │                                                      │
│ ▸ Preview        │                                                      │
│ ▸ Generate       │                                                      │
│ ▸ Settings       │                                                      │
└──────────────────┴──────────────────────────────────────────────────────┘
```

**Key Elements:**
- Topbar: Project name, Save status indicator, theme toggle, user menu (logout)
- Sidebar: 11 navigation entries (`packages/ide-core/src/components/Layout.tsx`),
  collapsible to icons
- Main Canvas: Primary editing area
- Toasts: bottom-right, driven by `uiStore.addNotification`
- Status Bar (Bottom): Project path, build status

---

## 3. Dashboard

The dashboard is **project-scoped**: it reports counts for the open project and
lists the projects the account owns. There is no activity timeline — the server
stores no audit log, so an "Updated Form at 10:30" row would have to be
invented.

### 3.1 Dashboard Wireframe

```text
┌────────────────────────────────────────────────────────────────────────────┐
│  Customer Portal                                     Unsaved changes        │
│  Customer-facing portal for support tickets                                 │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                            │
│  ┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐ ┌───────────┐│
│  │ Forms           │ │ Workflows       │ │ Data Models     │ │ Pages     ││
│  │ 8               │ │ 5               │ │ 10              │ │ 12        ││
│  └─────────────────┘ └─────────────────┘ └─────────────────┘ └───────────┘│
│                                                                            │
│  ┌───────────────────────┐ ┌───────────────────────┐                      │
│  │ Create Form           │ │ Design Workflow       │                      │
│  │ Build data collection │ │ Visual process        │                      │
│  │ forms                 │ │ automation            │                      │
│  └───────────────────────┘ └───────────────────────┘                      │
│  ┌───────────────────────┐ ┌───────────────────────┐                      │
│  │ Model Data            │ │ Build Pages           │                      │
│  └───────────────────────┘ └───────────────────────┘                      │
│                                                                            │
│  ┌─ Your projects ───────────────────────────────────────────────────────┐│
│  │ Customer Portal   customer-portal   updated 2 hours ago   [Open]      ││
│  │ HR Management     hr-management     updated 5 hours ago   [Open]      ││
│  └───────────────────────────────────────────────────────────────────────┘│
└────────────────────────────────────────────────────────────────────────────┘
```

**Components:**
- Header: project name and description; an "Unsaved changes" badge while the
  autosave debounce is pending
- Stats Cards: live counts from the open project (Forms, Workflows, Data
  Models, Pages), each a button that navigates to its builder
- Quick Actions: the four builder entry points
- Your projects: server-side project list with relative "last updated" and an
  Open action; the desktop host has no project list, so it shows an honest
  empty state instead of rows

---

## 4. Form Builder

### 4.1 Form Builder Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Form Builder - [Contact Form]            [Save] [Preview] [Generate Code] │
├─────────────────────────┬─────────────────────────────────────────────────────┤
│  Field Palette          │  Canvas Area                                         │
│                         │                                                     │
│  ┌─ Layout ─────────┐   │  ┌─ Toolbar ──────────────────────────────────────┐ │
│  │ Container         │   │  │ [Desktop] [Tablet] [Mobile] | [Grid] [Preview] │ │
│  │ Grid              │   │  └─────────────────────────────────────────────────┘ │
│  │ Card              │   │                                                     │
│  │ Section           │   │  ┌─────────────────────────────────────────────┐   │
│  └───────────────────┘   │  │ Contact Form                                 │   │
│  ┌─ Basic Input ─────┐   │  │ ─────────────────────────────────────────── │   │
│  │ Text Input        │   │  │ First Name *                                │   │
│  │ Textarea          │   │  │ [________________]                          │   │
│  │ Email             │   │  │                                             │   │
│  │ Password          │   │  │ Last Name *                                 │   │
│  │ Number            │   │  │ [________________]                          │   │
│  │ Date              │   │  │                                             │   │
│  │ File              │   │  │ Email *                                     │   │
│  └───────────────────┘   │  │ [________________]                          │   │
│  ┌─ Selection ───────┐   │  │                                             │   │
│  │ Select            │   │  │ [Submit]                                    │   │
│  │ Checkbox          │   │  └─────────────────────────────────────────────┘   │
│  │ Radio             │   │                                                     │
│  │ Toggle            │   │                                                     │
│  └───────────────────┘   │                                                     │
│  ┌─ Advanced ────────┐   │                                                     │
│  │ Rich Text         │   │                                                     │
│  │ Signature         │   │                                                     │
│  │ Computed          │   │                                                     │
│  │ Repeater          │   │                                                     │
│  └───────────────────┘   │                                                     │
├─────────────────────────┴─────────────────────────────────────────────────────┤
│                        Properties Panel                                      │
│  [General] [Validation] [Conditions] [Permissions] [Styling]                 │
│                                                                              │
│  ┌─ General ─────────────────────────────────────────────────────────────┐ │
│  │ Field Label: [First Name]                                               │ │
│  │ Field Name:  [first_name]                                               │ │
│  │ Placeholder: [Enter first name]                                         │ │
│  │ Required:    [✓]                                                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Drag & Drop UX:**
- Fields draggable from palette to canvas
- Visual drop zones highlighted
- Reorder via drag-handle
- Click field to select and show properties
- Delete button on hover
- Responsive device preview

---

## 5. Workflow Builder (BPMN)

### 5.1 Workflow Builder Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Workflow Builder - [Approval Flow]        [Save] [Debug] [Simulate] [Publish]│
├─────────────────────────┬─────────────────────────────────────────────────────┤
│  BPMN Nodes             │  React Flow Canvas                                 │
│                         │                                                     │
│  ┌─ Events ──────────┐   │  ┌─ Toolbar ──────────────────────────────────────┐ │
│  │ Start Event       │   │  │ [Zoom In] [Zoom Out] [Fit] [Pan] | [Grid On] │ │
│  │ End Event         │   │  └─────────────────────────────────────────────────┘ │
│  │ Intermediate      │   │                                                     │
│  └───────────────────┘   │  (o)─[Start]──(Exclusive)─[User Task]─(End)         │
│  ┌─ Tasks ───────────┐   │                │                                     │
│  │ User Task         │   │                └─[Manager Review]─(End)             │
│  │ Service Task      │   │                                                     │
│  │ Script Task       │   │                                                     │
│  │ Business Rule     │   │                                                     │
│  │ Send Task         │   │                                                     │
│  │ Receive Task      │   │                                                     │
│  └───────────────────┘   │                                                     │
│  ┌─ Gateways ────────┐   │                                                     │
│  │ Exclusive         │   │                                                     │
│  │ Inclusive         │   │                                                     │
│  │ Parallel          │   │                                                     │
│  └───────────────────┘   │                                                     │
│  ┌─ Subprocess ──────┐   │                                                     │
│  │ Subprocess        │   │                                                     │
│  └───────────────────┘   │                                                     │
├─────────────────────────┴─────────────────────────────────────────────────────┤
│                        Properties Panel                                      │
│  [General] [Assignment] [Conditions] [Timers] [Data Mapping]                  │
│                                                                              │
│  ┌─ General ─────────────────────────────────────────────────────────────┐ │
│  │ Node ID:      [userTask_1]                                             │ │
│  │ Node Name:    [Manager Approval]                                        │ │
│  │ Description:  [Requires manager approval for > $1000]                   │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│                                                                              │
│  ┌─ Assignment ──────────────────────────────────────────────────────────┐ │
│  │ Assignee:     [manager@company.com] or [Expression ▼]                   │ │
│  │ Candidate Groups: [managers, approvers]                                │ │
│  │ Due Date:     [+ Set due date]                                          │ │
│  │ Form Link:    [Approval Form ▼]                                        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**BPMN Visual Elements:**
- Drag nodes onto canvas
- Connect with edges (click+drag)
- Right-click context menu (Delete, Copy, Properties)
- Selected node highlighted with blue border
- Exclusive gateway shows condition labels on edges
- Debug mode shows execution path (green highlighted)

---

## 6. Data Modeler

### 6.1 Data Modeler Layout (ERD)

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Data Modeler - [Customer Portal]          [Save] [SQL Preview] [Migrate]    │
├─────────────────────────┬─────────────────────────────────────────────────────┤
│  Entity Palette         │  ERD Canvas                                        │
│                         │                                                     │
│  ┌─ New Entity ──────┐   │  ┌─ Toolbar ──────────────────────────────────────┐ │
│  │ [Add Entity]       │   │  │ [Zoom] [Fit View] [Auto Layout] | [Show FK]   │ │
│  └───────────────────┘   │  └─────────────────────────────────────────────────┘ │
│  ┌─ Entities List ────┐   │                                                     │
│  │ • users            │   │  ┌─────────────────┐      ┌─────────────────┐     │
│  │ • orders           │   │  │ users           │─────│ orders          │     │
│  │ • products         │   │  │ ─────────────── │1    │ ─────────────── │     │
│  │ • order_items      │   │  │ id (PK, uuid)   │*    │ id (PK, uuid)   │     │
│  │ • categories       │   │  │ email (string)  │     │ user_id (FK)    │     │
│  └───────────────────┘   │  │ name (string)   │     │ total (decimal) │     │
│                         │  │ created_at      │     │ status (enum)   │     │
│                         │  └─────────────────┘      └─────────────────┘     │
│                         │            │                        │               │
│                         │            │                        │1              │
│                         │            └─────────────┬────────────┘               │
│                         │                          │*                          │
│                         │                    ┌─────────────────┐                │
│                         │                    │ order_items     │                │
│                         │                    │ ─────────────── │                │
│                         │                    │ id (PK, uuid)   │                │
│                         │                    │ order_id (FK)   │                │
│                         │                    │ product_id (FK) │                │
│                         │                    │ quantity (int)  │                │
│                         │                    └─────────────────┘                │
│                         │                          │1                            │
│                         │                          │                             │
│                         │                          │*                            │
│                         │                    ┌─────────────────┐                │
│                         │                    │ products        │                │
│                         │                    │ ─────────────── │                │
│                         │                    │ id (PK, uuid)   │                │
│                         │                    │ name (string)   │                │
│                         │                    │ price (decimal) │                │
│                         │                    └─────────────────┘                │
├─────────────────────────┴─────────────────────────────────────────────────────┤
│                        Entity Properties                                      │
│  [Fields] [Relations] [Indexes] [Constraints] [Permissions] [RLS]           │
│                                                                              │
│  ┌─ Fields ───────────────────────────────────────────────────────────────┐ │
│  │ + Add Field                                                              │ │
│  │                                                                          │ │
│  │ Name      | Type      | PK | Null | Unique | Default | Actions          │ │
│  │ ----------|-----------|----|------|--------|---------|---------         │ │
│  │ id        | uuid      | ✓  | ✗    | ✓      | uuid()  | [Edit][Del]      │ │
│  │ email     | string    | ✗  | ✗    | ✓      | -       | [Edit][Del]      │ │
│  │ name      | string    | ✗  | ✓    | ✗      | -       | [Edit][Del]      │ │
│  │ created_at| timestamp | ✗  | ✗    | ✗      | now()   | [Edit][Del]      │ │
│  │ updated_at| timestamp | ✗  | ✓    | ✗      | now()   | [Edit][Del]      │ │
│  └──────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**ERD Features:**
- Visual entities as cards
- Relationships shown as connecting lines with cardinality (1:N)
- Click entity to edit fields
- Add/Remove fields inline
- Field types: string, text, integer, bigint, float, decimal, boolean, date, datetime, timestamp, json, jsonb, uuid, enum
- SQL Preview modal shows DDL

---

## 7. Page Builder

### 7.1 Page Builder Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Page Builder - [Home Page]                [Save] [Preview] [Publish]        │
├─────────────────────────┬─────────────────────────────────────────────────────┤
│  Component Library      │  Page Canvas                                      │
│                         │                                                     │
│  ┌─ Layout ──────────┐   │  ┌─ Device Toolbar ──────────────────────────────┐ │
│  │ Container          │   │  │ [Desktop 1440] [Tablet 768] [Mobile 375]      │ │
│  │ Grid               │   │  └─────────────────────────────────────────────────┘ │
│  │ Flex               │   │                                                     │
│  │ Card               │   │  ┌─────────────────────────────────────────────┐   │
│  │ Tabs               │   │  │ HEADER                                       │   │
│  └───────────────────┘   │  │ ─────────────────────────────────────────── │   │
│  ┌─ Data Display ────┐   │  │ Nav: Home | Products | About | Contact       │   │
│  │ Table              │   │  └─────────────────────────────────────────────┘   │
│  │ List               │   │                                                     │
│  │ Chart              │   │  ┌─────────────────────────────────────────────┐   │
│  │ StatCard           │   │  │ HERO SECTION                                 │   │
│  │ Progress           │   │  │ Welcome to Customer Portal                  │   │
│  └───────────────────┘   │  │ [Get Started]                                │   │
│  ┌─ Input ───────────┐   │  └─────────────────────────────────────────────┘   │
│  │ Button             │   │                                                     │
│  │ Input              │   │  ┌─ Stats Row ────────────────────────────────┐   │
│  │ Select             │   │  │ [Total Orders: 1,234] [Revenue: $45K]      │   │
│  │ Form               │   │  └─────────────────────────────────────────────┘   │
│  └───────────────────┘   │                                                     │
│  ┌─ Navigation ──────┐   │  ┌─ Recent Orders Table ──────────────────────┐   │
│  │ Menu               │   │  │ Order ID | Date | Status | Amount           │   │
│  │ Breadcrumb         │   │  │ ----------|------|--------|---------         │   │
│  │ Pagination         │   │  │ ORD-001  | Oct  | Paid   | $120.00          │   │
│  └───────────────────┘   │  │ ORD-002  | Oct  | Pending| $89.50           │   │
│  ┌─ Feedback ────────┐   │  └─────────────────────────────────────────────┘   │
│  │ Alert              │   │                                                     │
│  │ Modal              │   │                                                     │
│  └───────────────────┘   │                                                     │
├─────────────────────────┴─────────────────────────────────────────────────────┤
│                        Properties Panel                                      │
│  [Style] [Data Binding] [Events] [Conditions] [Permissions]                  │
│                                                                              │
│  ┌─ Style ────────────────────────────────────────────────────────────────┐ │
│  │ Padding:  [16px 24px]                                                    │ │
│  │ Margin:   [0px 0px 24px 0px]                                             │ │
│  │ Background: [#ffffff ▼]                                                 │ │
│  │ Border Radius: [8px ▼]                                                   │ │
│  └──────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Page Builder UX:**
- Components draggable to canvas
- Tree view of component hierarchy in sidebar (left alternative)
- Device toggle: Desktop/Tablet/Mobile with accurate widths
- Live preview mode available
- Nested containers with visual hierarchy
- Data binding panel maps to API/entities
- Event system configures actions on click/change
- **List Builder**: entity-driven Table/List with columns, filters, sorting, pagination, actions

---

## 8. Theme Builder

### 8.1 Theme Builder Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Theme Builder - [Default Theme]           [Save] [Export] [Apply]           │
├─────────────────────────┬─────────────────────────────────────────────────────┤
│  Design Tokens          │  Live Preview                                     │
│                         │                                                     │
│  ┌─ Colors ──────────┐   │  ┌─ Preview Toolbar ────────────────────────────┐ │
│  │ Primary            │   │  │ [Buttons] [Forms] [Cards] [Typography] [All] │ │
│  │ Secondary          │   │  └─────────────────────────────────────────────────┘ │
│  │ Success            │   │                                                     │
│  │ Warning            │   │  ┌─ Preview: Buttons ──────────────────────────┐   │
│  │ Error              │   │  │ Primary:   [Primary Button]                  │   │
│  │ Neutral            │   │  │ Secondary: [Secondary Button]                │   │
│  │ Background         │   │  │ Success:   [Success Button]                  │   │
│  └───────────────────┘   │  │ Danger:    [Danger Button]                   │   │
│  ┌─ Typography ──────┐   │  │ Outline:   [Outline Button]                  │   │
│  │ Font Family        │   │  └─────────────────────────────────────────────┘   │
│  │ Font Sizes         │   │                                                     │
│  │ Font Weights       │   │  ┌─ Preview: Card ─────────────────────────────┐   │
│  │ Line Heights       │   │  │ ┌─────────────────────────────────────────┐ │   │
│  └───────────────────┘   │  │ │ Card Title                               │ │   │
│  ┌─ Spacing ─────────┐   │  │ │ This is card content with default       │ │   │
│  │ Base Unit          │   │  │ │ spacing and styling.                    │ │   │
│  │ Scale              │   │  │ └─────────────────────────────────────────┘ │   │
│  └───────────────────┘   │  └─────────────────────────────────────────────┘   │
│  ┌─ Borders ─────────┐   │                                                     │
│  │ Radius             │   │  ┌─ Color Picker ──────────────────────────────┐   │
│  │ Width              │   │  │ #2563eb                                      │   │
│  └───────────────────┘   │  │ [Color Swatch] [HEX] [RGB] [HSL]             │   │
│  ┌─ Shadows ─────────┐   │  └─────────────────────────────────────────────┘   │
│  │ Elevation          │   │                                                     │
│  └───────────────────┘   │                                                     │
│  ┌─ Advanced ────────┐   │                                                     │
│  │ Dark Mode          │   │                                                     │
│  │ CSS Variables      │   │                                                     │
│  └───────────────────┘   │                                                     │
├─────────────────────────┴─────────────────────────────────────────────────────┤
│  Tailwind Config Preview:                                                     │
│  ┌────────────────────────────────────────────────────────────────────────────┐ │
│  │ theme: { extend: { colors: { primary: '#2563eb', ... } } }               │ │
│  └────────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Theme Features:**
- Real-time color picker with preview
- Design token editor with grouped categories
- Live component preview across multiple variants
- Dark mode toggle with strategy selector
- One-click export to CSS variables & Tailwind config
- Copy CSS snippet

---

## 9. Plugin Manager

There is **no marketplace** and no plugin registry: a plugin is a
`plugin.json` manifest plus a `.wasm` module the user already has, and the IDE
server owns the sandbox it runs in. The screen therefore has one tab-shaped
job — install from files, then enable, disable or remove.

### 9.1 Plugin Manager Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Plugin Manager                                                                  │
├────────────────────────────────────────────────────────────────────────────────┤
│  There is no marketplace: the IDE installs what you give it.                     │
├────────────────────────────────────────────────────────────────────────────────┤
│  ┌─ Install ─────────────────────────────────────────────────────────────────┐ │
│  │  [Choose plugin.json…]  [Choose plugin.wasm…]            [Install]        │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  Search: [Search installed plugins...]                                           │
│                                                                                  │
│  ┌─ Plugin Row ─────────────────────────────────────────────────────────────┐ │
│  │ [📦] com.acme.charts   v1.2.3   by acme        [Enabled ▢]  [🗑 Remove]  │ │
│  │      Charts for pages (line, bar, pie).                                  │ │
│  │      Permissions: read_pages, write_pages                                │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│  ┌─ Plugin Row ─────────────────────────────────────────────────────────────┐ │
│  │ [📦] com.acme.validator v0.9.0  by acme        [Disabled ▢] [🗑 Remove]  │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Plugin Manager States:**
- **Empty**: "No plugins installed yet", with the install row still visible
- **Unsupported host**: the desktop shell has no plugin sandbox, so the screen
  says plugins are unavailable instead of offering a form that cannot work
- **Install**: manifest is parsed and validated server-side (id, semver,
  `api_version`, duplicate permissions and entry points) before the module is
  accepted
- **Per user**: installed plugins are scoped to the account, so two users can
  install the same plugin id independently

---

## 10. Code Generator

Codegen always emits the backend, frontend, database, docker and kubernetes
trees, so there are **no target checkboxes** — a checkbox that no code path
reads is a lie about what the button does. The screen configures the two things
that actually change the output (output directory and database driver) and
reports what was written.

### 10.1 Code Generator Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Code Generator - [Customer Portal]                        [▶ Generate Code]   │
├──────────────────────────────┬─────────────────────────────────────────────────┤
│  Generation Config           │  Output                            [Ready]      │
│                              │                                                 │
│  Output Directory            │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐   │
│  [./generated      ] [📂]    │  │  24    │ │  18    │ │   3    │ │   2    │   │
│  The generated backend is    │  │backend │ │frontend│ │  db    │ │docker  │   │
│  compiled against this       │  │ files  │ │ files  │ │ files  │ │ files  │   │
│  driver, so preview uses it  │  └────────┘ └────────┘ └────────┘ └────────┘   │
│                              │                                                 │
│  Database                    │  ┌─ Log ─────────────────────────────────────┐ │
│  [SQLite ▼]                  │  │ [10:45:23] Starting code generation…      │ │
│                              │  │ [10:45:24] Generating backend models…     │ │
│  ┌─ What will be generated ─┐│  │ [10:45:25] Generating API routes…         │ │
│  │ Entities            10   ││  │ [10:45:26] Generating frontend…           │ │
│  │ Forms                8   ││  │ [10:45:27] Generating migrations…         │ │
│  │ Pages               12   ││  │ [10:45:28] Generation completed           │ │
│  └──────────────────────────┘│  └───────────────────────────────────────────┘ │
│  Codegen emits the backend,  │                                                 │
│  frontend, database, docker  │                                                 │
│  and kubernetes trees for    │                                                 │
│  every run.                  │                                                 │
└──────────────────────────────┴─────────────────────────────────────────────────┘
```

**Code Generator Features:**
- Output directory with a folder picker (native dialog on the desktop host)
- Database driver, which the generated backend is compiled against
- "What will be generated": live counts from the project model, with an
  honest empty state when the project has no entities, forms or pages
- Run report: per-tree file counts read from the generator's own report
- Generation log with timestamps, cancellable while running
- Status pill: Idle / Generating / Ready / Failed

---

## 11. Project Settings

### 11.1 Project Settings Tabs

Three tabs, because three groups of settings change something. Auth, email,
API, storage, notification and feature-flag inputs were removed: nothing in
codegen reads them, so editing them looked like configuration while changing
nothing.

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Project Settings - [Customer Portal]                                           │
├────────────────────────────────────────────────────────────────────────────────┤
│  [General] [Database] [Frontend]                                     [Save]     │
├────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─ General ───────────────────────────────────────────────────────────────┐  │
│  │ Project Name:  [Customer Portal]                                         │  │
│  │                Renaming also rewrites the slug, which the generated       │  │
│  │                crate name is derived from.                               │  │
│  │ Slug:          [customer-portal]        (read-only)                      │  │
│  │ Description:   [Customer self-service portal                    ]        │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
│                                                                                  │
│  ┌─ Database ──────────────────────────────────────────────────────────────┐  │
│  │ Driver:  [SQLite ▼]                                                      │  │
│  │ URL:     [sqlite://app.db?mode=rwc                            ]          │  │
│  │ ⚠ Leave the URL empty to use a throwaway database instead.              │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
│                                                                                  │
│  ┌─ Frontend ──────────────────────────────────────────────────────────────┐  │
│  │ Framework: [Leptos (Rust / WASM) ▼]                                      │  │
│  │            Generated frontends are Leptos, compiled to WebAssembly.      │  │
│  │ Bundler:   [Trunk ▼]                        (disabled)                   │  │
│  └──────────────────────────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Settings Tabs Coverage:**
- **General**: name, derived slug (read-only), description
- **Database**: driver, connection URL, throwaway-database mode
- **Frontend**: framework (Leptos only) and bundler (Trunk, shown disabled)

Auto-save is not a setting: it is always on, debounced 1500 ms after the last
change. Export is a Dashboard/Projects action, not a settings field.

---

## 12. Modal Examples

### 12.1 Save Project

Saving is not a modal. The topbar `Save` button writes the project immediately,
bypassing the 1500 ms autosave debounce; when the project has never been saved
it falls through to `saveProjectAs`, which uses the host's native save dialog
(`pickSavePath`) rather than an in-app modal. On the web host the same path is a
server write, so no file picker appears.

### 12.2 BPMN 2.0 Import Modal

```text
┌───────────────────────────────────────────────────────────────────────┐
│  Import BPMN 2.0                                                  ✕   │
├───────────────────────────────────────────────────────────────────────┤
│  [Process name                                                  ]     │
│  ┌─────────────────────────────────────────────────────────────────┐  │
│  │ <definitions xmlns="http://www.omg.org/spec/BPMN/20100524/MODEL"│  │
│  │              targetNamespace="http://bpmn.io/schema/bpmn">      │  │
│  │   <process id="Process_1" isExecutable="false">                 │  │
│  │     ...                                                         │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│  ⚠ Line 12: unknown element <boundaryEvent> — skipped                │
│                                                                       │
│                                        [Cancel]  [Import]             │
└───────────────────────────────────────────────────────────────────────┘
```

Parsed server-side by `crates/bpmn/src/xml.rs`; a parse error is reported in
place and the dialog stays open. Import never overwrites an existing workflow —
it always creates a new one with the name in the first field.

### 12.3 Unsaved Changes Guard

The guard is a native `window.confirm`, not a custom modal: it has to work from
a `beforeunload` handler and from the sign-out path, and both need a
synchronous answer. It fires only while `isDirty` is true.

```text
┌─────────────────────────────────────────────┐
│  This page says                          ✕  │
├─────────────────────────────────────────────┤
│  You have unsaved changes. Leave this       │
│  page and lose them?                        │
│                                               │
│  [Cancel]  [OK]                             │
└─────────────────────────────────────────────┘
```

Triggers: in-app navigation to the projects screen, sign-out (both the sidebar
user menu and the projects screen), and browser tab close. Every other dialog
uses the shared `Modal` component, which closes on Escape, moves focus into the
dialog on open, returns it to the trigger on close, wraps Tab inside the dialog
and carries `role="dialog" aria-modal="true"`.

### 12.4 Web IDE - Sign In

A full page, not a modal: the web IDE has no shell to sit on until you are
authenticated. There is no "remember me" (the refresh cookie is HttpOnly and
already long-lived) and no "forgot password" (there is no mail transport in the
server, so a reset link could not be sent — offering the link would be a dead
end).

```text
┌─────────────────────────────────────────────┐
│  Bonafoid                                   │
│  Sign in to your workspace                  │
├─────────────────────────────────────────────┤
│  Email     [user@example.com]               │
│  Password  [••••••••]                        │
│  ⚠ Invalid email or password                │
│                                               │
│  [            Sign in                      ] │
├─────────────────────────────────────────────┤
│  No account? Create one                     │
└─────────────────────────────────────────────┘
```

### 12.5 Web IDE - Create Account

```text
┌─────────────────────────────────────────────┐
│  Create your account                        │
│  Projects are stored server-side and        │
│  scoped to you                              │
├─────────────────────────────────────────────┤
│  Email             [user@example.com]        │
│  Password          [••••••••]                │
│                    At least 8 characters.    │
│  Confirm password  [••••••••]                │
│                                               │
│  [         Create account                  ] │
├─────────────────────────────────────────────┤
│  Already registered? Sign in                │
└─────────────────────────────────────────────┘
```

Both screens share `AuthForm`, so validation and error presentation are
identical: mismatched passwords and short passwords are caught client-side, and
the server's error message is shown in a `role="alert"` paragraph.

### 12.6 Web IDE - Projects Screen

A single-column list, not a card grid: each row is a project you can open,
duplicate, export or delete. New project is an inline row at the top rather than
a button that opens a dialog.

```text
┌────────────────────────────────────────────────────────────────┐
│  Projects                                        [⏻ Sign out]  │
├────────────────────────────────────────────────────────────────┤
│  [New project name                        ]  [+ Create]        │
│                                                                │
│  ⬆ Import project                                              │
│                                                                │
│  📄 Customer Portal      customer-portal    2 hours ago        │
│                                       [Open] [⧉] [⬇] [🗑]      │
│  📄 HR Management        hr-management      5 hours ago        │
│                                       [Open] [⧉] [⬇] [🗑]      │
│  📄 E-commerce App       e-commerce-app     3 days ago         │
│                                       [Open] [⧉] [⬇] [🗑]      │
└────────────────────────────────────────────────────────────────┘
```

Row actions are icon buttons with `title` and `aria-label`: Open, Duplicate,
Export (downloads the project JSON), Delete. Delete and Sign out both run
through the unsaved-changes guard.

---

## 13. Responsive Breakpoints

| Device | Width | Canvas Padding | Notes |
|--------|-------|----------------|-------|
| Desktop | 1440px | 40px | Default IDE canvas |
| Laptop | 1280px | 32px | Common screen size |
| Tablet Landscape | 1024px | 24px | Page builder preview |
| Tablet Portrait | 768px | 20px | Page builder preview |
| Mobile | 375px | 16px | Page builder preview |
| Mobile Small | 320px | 12px | Edge case |

---

## 14. Component States

| Component | Default | Hover | Active | Focus | Disabled | Loading |
|-----------|---------|-------|--------|-------|----------|---------|
| Button | Solid bg | Darker bg | Pressed | Ring | Gray bg | Spinner |
| Input | Border | Border color | - | Ring | Gray bg | - |
| Card | Shadow | Shadow-lg | - | - | Opacity-50 | - |
| Dropdown | Closed | - | Open | - | Disabled | - |
| Tab | Inactive | Hover bg | Active | Ring | - | - |

---

## 15. Implementation Notes

- **Framework**: React 18 + TypeScript + Tailwind CSS 3, shared through `packages/ide-core`
- **Drag & Drop**: native HTML5 drag events (`draggable` + `onDragStart`/`onDrop`)
  in the form field list — no @dnd-kit dependency; reordering is also reachable
  from the keyboard
- **Flow Editor**: React Flow (`reactflow`) for the BPMN workflow builder
- **Canvas Rendering**: SVG + HTML for the ERD in the data modeler
- **Icons**: Lucide React (consistent icon set)
- **Animations**: Tailwind transitions (150–300 ms)
- **State**: zustand stores (`projectStore`, `pluginStore`, `uiStore`)
- **Routing**: react-router-dom v6 `BrowserRouter` — not a data router, so
  navigation blocking uses a `beforeunload` guard plus an explicit
  `confirmLeave()` call, not `useBlocker`
- **Auto-save**: debounced 1500 ms after the last change; a failure raises a
  toast, and leaving a dirty project prompts first
- **Toast Notifications**: the shared `uiStore` notification surface driven by
  `describeError`/`notifyError` — no third-party toast library
- **Modals**: the hand-rolled `components/ui/Modal.tsx` (no Radix dependency);
  Escape to close, focus moved in and restored, Tab wrapped
- **Tooltips**: `title` attributes plus inline hints
- **Code display**: plain `<pre>` blocks — there is no Monaco dependency and no
  SQL preview modal

### 15.1 Accessibility (as implemented)

The design target is WCAG 2.1 AA. What the code actually does today:

| Concern | Implementation |
|---------|----------------|
| Canvas nodes | `role="button"`, `tabIndex={0}`, `aria-selected`, Enter/Space activation |
| Reordering | The drag handle is a real `<button>` with an accessible name and ↑/↓ keys, not a decorative `div` |
| Icon-only buttons | `aria-label` on every one; icons carry `aria-hidden` so the name is not read twice |
| Disclosures | `aria-expanded` on the options toggle and the New Page trigger |
| Focus management | The shared `Modal` moves focus in on open, returns it to the trigger on close and wraps Tab; inline forms close on Escape and return focus to the button that opened them |
| Theme contrast | `readableTextColor()` picks white or near-black per background; `themeModel.test.ts` asserts ≥ 4.5:1 for every colour role and for body/secondary/tertiary text |
| Page structure | Process nodes are `role="group"` with a label, so a screen reader announces the node's identity before its controls |

---

**Document Control**

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-15 | UX Designer | Initial Draft - Complete UI/UX wireframes & mockups for No-Code IDE platform |
| 1.1 | 2026-10-16 | Engineering | Add Web IDE auth + project dashboard, new List Builder wireframe; note web flows |
| 1.2 | 2026-10-28 | Engineering | §15 rewritten to match the shipped dependency set (no Sonner/Radix); §15.1 added with the accessibility work actually implemented |
| 1.3 | 2026-10-29 | Engineering | Re-drew §2, §3, §9, §10, §11, §12 against the shipped screens: sidebar not topbar tabs, no marketplace, no target checkboxes, 3-tab settings, no activity timeline; §12.3 added (the gap); §15 corrected again (no @dnd-kit, no Monaco, autosave is 1500 ms); Modal gained focus management |