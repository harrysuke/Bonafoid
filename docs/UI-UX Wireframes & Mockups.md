# UI/UX Wireframes & Mockups

## No-Code Visual Application Development Platform

**Version:** 1.0  
**Date:** October 2026  
**Status:** Draft  

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

```text
┌─────────────────────────────────────────────────────────────────────────┐
│  [Logo] No-Code IDE                                    [Save] [Preview] │
├─────────────────────────────────────────────────────────────────────────┤
│  [1. Dashboard] [2. Forms] [3. Workflow] [4. Data] [5. Pages] [6. Theme] │
│  [7. Plugins] [8. Generator] [9. Settings]                             │
├─────────────────────────┬─────────────────────────────────────────────────┤
│  Sidebar (Contextual)   │  Main Canvas / Editor Area                    │
│  [Palette/Tree/Props]  │  [Toolbar] [Content] [Bottom Panel]            │
│                         │                                                 │
└─────────────────────────┴─────────────────────────────────────────────────┘
```

**Key Elements:**
- Topbar: Project name, Save status indicator, Global actions (Save, Preview, Export, Run)
- Main Navigation Tabs: Horizontal tabs for 9 major modules
- Contextual Sidebar: Dynamic based on selected module
- Main Canvas: Primary editing area
- Status Bar (Bottom): Project path, build status, cursor position

---

## 3. Dashboard

### 3.1 Dashboard Wireframe

```text
┌────────────────────────────────────────────────────────────────────────────┐
│  Dashboard                                                                     │
├────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  Welcome back! Let's start building your application.                      │
│                                                                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐  ┌──────────┐ │
│  │ Projects        │  │ Forms           │  │ Workflows       │  │ Entities  │ │
│  │ 12 Active       │  │ 8 Created       │  │ 5 Active        │  │ 10 Models │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘  └──────────┘ │
│                                                                              │
│  ┌───────────────────────────────────┐  ┌─────────────────────────────────┐│
│  │ Quick Actions                    │  │ Recent Projects                 ││
│  │                                 │  │                                 ││
│  │ [New Form] [New Workflow]       │  │ • Customer Portal • 2h ago     ││
│  │ [New Data Model] [New Page]     │  │ • HR Management  • 5h ago     ││
│  │ [Generate Code] [Deploy]        │  │ • E-commerce App • Yesterday  ││
│  │                                 │  │ • Task Manager   • 3 days ago││
│  └───────────────────────────────────┘  └─────────────────────────────────┘│
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐│
│  │ Recent Activity                                                         ││
│  │ • Updated Form: Contact Form - 10:30 AM                               ││
│  │ • Modified Workflow: Approval Flow - 9:15 AM                           ││
│  │ • Generated Code for Customer Portal - 8:00 AM                         ││
│  └────────────────────────────────────────────────────────────────────────┘│
└────────────────────────────────────────────────────────────────────────────┘
```

**Components:**
- Stats Cards: Key metrics (Projects, Forms, Workflows, Entities)
- Quick Actions: One-click shortcuts to common tasks
- Recent Projects: List with name, last modified
- Recent Activity: Timeline of user actions

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

### 9.1 Plugin Manager Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Plugin Manager                                                                  │
├────────────────────────────────────────────────────────────────────────────────┤
│  [Installed (12)] [Marketplace (45)] [Develop (My Plugins)]                      │
├────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─ Filters ─────────────────────────────────────────────────────────────────┐ │
│  │ Search: [Search plugins...]  Category: [All ▼]  Sort: [Popular ▼]      │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  ┌─ Plugin Card ──────────────┐  ┌─ Plugin Card ──────────────┐             │
│  │                             │  │                             │             │
│  │ [📦] Form Validator         │  │ [📦] Chart Components       │             │
│  │ by nocode-team • v1.2.3     │  │ by data-viz • v2.0.1        │             │
│  │ Rating: ★★★★☆ (234)        │  │ Rating: ★★★★★ (892)         │             │
│  │                             │  │                             │             │
│  │ Add validation rules to     │  │ Beautiful charts for pages  │             │
│  │ your forms with ease.       │  │ (Line, Bar, Pie, Scatter).  │             │
│  │                             │  │                             │             │
│  │ [Install] [Details]         │  │ [Install] [Details]         │             │
│  └─────────────────────────────┘  └─────────────────────────────┘             │
│                                                                                  │
│  ┌─ Plugin Card ──────────────┐  ┌─ Plugin Card ──────────────┐             │
│  │                             │  │                             │             │
│  │ [📦] AWS S3 Storage         │  │ [📦] Email Templates        │             │
│  │ by cloud-team • v1.0.5      │  │ by mail-team • v1.1.0       │             │
│  │ Rating: ★★★★☆ (89)         │  │ Rating: ★★★★☆ (156)         │             │
│  │                             │  │                             │             │
│  │ S3 storage driver for       │  │ Custom email templates      │             │
│  │ uploads and assets.         │  │ with MJML support.          │             │
│  │                             │  │                             │             │
│  │ [Install] [Details]         │  │ [Install] [Details]         │             │
│  └─────────────────────────────┘  └─────────────────────────────┘             │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Plugin Manager States:**
- **Installed Tab**: Enable/Disable toggle, Configure, Update, Uninstall
- **Marketplace Tab**: Search, filter by category (Component, Action, Workflow, Storage, Auth), install
- **Develop Tab**: "Create New Plugin" button, scaffold project, test locally, publish

---

## 10. Code Generator

### 10.1 Code Generator Layout

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Code Generator - [Customer Portal]      [Generate] [Build] [Preview]         │
├────────────────────────────────────────────────────────────────────────────────┤
│  Targets & Configuration                                                        │
│                                                                                  │
│  ┌─ Target Selection ──────────────────────────────────────────────────────┐ │
│  │ [✓] Web (Leptos + WASM + PWA)                                            │ │
│  │ [✓] Desktop (Tauri v2)                                                   │ │
│  │ [✓] Docker (Multi-stage)                                                 │ │
│  │ [✓] Kubernetes (Manifests)                                               │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  ┌─ Environment Configuration ──────────────────────────────────────────────┐ │
│  │ Database:  [PostgreSQL ▼]                                                │ │
│  │ URL:       [postgresql://user:pass@localhost:5432/db]                   │ │
│  │ Features:  [✓] Auth [✓] API [✓] Workflows [✓] Files [✓] Audit Log      │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  ┌─ Generation Preview ────────────────────────────────────────────────────┐ │
│  │ Output Directory: [D:\nocode-ide\generated\customer-portal]              │ │
│  │ [Browse...]                                                               │ │
│  │                                                                            │ │
│  │ Generated Structure:                                                       │ │
│  │ ├── backend/           (Rust/Axum)                                        │ │
│  │ │   ├── src/                                                              │ │
│  │ │   ├── Cargo.toml                                                        │ │
│  │ │   └── migrations/                                                       │ │
│  │ ├── frontend/          (Leptos/WASM)                                      │ │
│  │ │   ├── src/                                                              │ │
│  │ │   ├── Cargo.toml                                                        │ │
│  │ │   ├── package.json                                                      │ │
│  │ │   └── tailwind.config.js                                               │ │
│  │ ├── database/         (SQL)                                               │ │
│  │ ├── docker/            (Dockerfiles)                                      │ │
│  │ ├── kubernetes/        (K8s YAML)                                         │ │
│  │ └── README.md                                                             │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  ┌─ Generation Log ─────────────────────────────────────────────────────────┐ │
│  │ [10:45:23] Starting code generation...                                   │ │
│  │ [10:45:24] Generating backend models...                                   │ │
│  │ [10:45:25] Generating API routes...                                      │ │
│  │ [10:45:26] Generating frontend components...                             │ │
│  │ [10:45:27] Generating database migrations...                             │ │
│  │ [10:45:28] Generation completed successfully!                            │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Code Generator Features:**
- Multi-target selection with checkboxes
- Feature flags for optional modules
- Real-time project structure preview
- Generation progress with timestamps
- Build button to compile generated code
- Preview button to run locally
- Open in Explorer/Finder

---

## 11. Project Settings

### 11.1 Project Settings Tabs

```text
┌────────────────────────────────────────────────────────────────────────────────┐
│  Project Settings - [Customer Portal]                                           │
├────────────────────────────────────────────────────────────────────────────────┤
│  [General] [Database] [Auth] [Email] [API] [Storage] [Features] [Notifications] │
├────────────────────────────────────────────────────────────────────────────────┤
│                                                                                  │
│  ┌─ General Settings ──────────────────────────────────────────────────────┐ │
│  │ Project Name:     [Customer Portal]                                      │ │
│  │ Project Slug:     [customer-portal]                                      │ │
│  │ Description:      [Customer self-service portal]                        │ │
│  │ Version:          [1.0.0]                                                │ │
│  │ Default Locale:   [English (en) ▼]                                       │ │
│  │ Timezone:         [UTC ▼]                                                 │ │
│  │ Currency:         [USD ▼]                                                 │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                                  │
│  ┌─ Advanced ────────────────────────────────────────────────────────────────┐ │
│  │ Auto-save:        [✓] Every 30 seconds                                   │ │
│  │ Backup on save:   [✓] Enabled                                            │ │
│  │ Git integration:   [ ] Enable Git                                         │ │
│  │ Export format:    [JSON ▼]                                                │ │
│  └─────────────────────────────────────────────────────────────────────────┘ │
└────────────────────────────────────────────────────────────────────────────────┘
```

**Settings Tabs Coverage:**
- **General**: Metadata, locale, timezone, formatting
- **Database**: Driver, connection, pool, SSL
- **Authentication**: JWT, MFA, Social OAuth, SSO
- **Email**: SMTP/config, templates, from address
- **API**: Base path, versioning, CORS, rate limit
- **Storage**: Local/S3/GCS/Azure, limits
- **Features**: 16 feature toggles
- **Notifications**: Channels & templates

---

## 12. Modal Examples

### 12.1 Save Project Modal

```text
┌─────────────────────────────────────────────┐
│  Save Project                                 │
├─────────────────────────────────────────────┤
│                                               │
│  Project Name: [Customer Portal]             │
│  Location:     [D:\projects\]                 │
│                [Browse...]                   │
│                                               │
│  [Save] [Cancel]                             │
└─────────────────────────────────────────────┘
```

### 12.2 SQL Preview Modal

```text
┌───────────────────────────────────────────────────────────────────────┐
│  SQL Preview - users table                                            │
├───────────────────────────────────────────────────────────────────────┤
│  -- Create table: users                                                │
│  CREATE TABLE users (                                                  │
│    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),                      │
│    email VARCHAR(255) NOT NULL UNIQUE,                                 │
│    name VARCHAR(255),                                                  │
│    created_at TIMESTAMP NOT NULL DEFAULT NOW(),                       │
│    updated_at TIMESTAMP DEFAULT NOW()                                  │
│  );                                                                     │
│                                                                         │
│  CREATE UNIQUE INDEX idx_users_email ON users(email);                 │
│                                                                         │
│  [Copy to Clipboard] [Download .sql] [Close]                           │
└───────────────────────────────────────────────────────────────────────┘
```

### 12.4 Web IDE - Login Modal

```text
┌─────────────────────────────────────────────┐
│  Sign in to Web IDE                         │
├─────────────────────────────────────────────┤
│  Email:    [user@example.com]               │
│  Password: [••••••••]                        │
│  [✓] Remember me                            │
│  [Forgot password?]                         │
│                                               │
│  [Sign in]  [Create account]                │
└─────────────────────────────────────────────┘
```

### 12.5 Web IDE - Register Modal

```text
┌─────────────────────────────────────────────┐
│  Create your account                        │
├─────────────────────────────────────────────┤
│  Email:           [user@example.com]         │
│  Password:        [••••••••]                 │
│  Confirm Password:[••••••••]                 │
│  By creating an account you agree to T&Cs   │
│                                               │
│  [Create account]  [Sign in instead]        │
└─────────────────────────────────────────────┘
```

### 12.6 Web IDE - Project Dashboard

```text
┌────────────────────────────────────────────────────────────────┐
│  Web IDE  [User ▼]  [New Project]  [Import]  [Logout]        │
├────────────────────────────────────────────────────────────────┤
│  My Projects (12)                                              │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
│  │ Customer Portal │  │ HR Management  │  │ E-commerce App │ │
│  │ Last modified   │  │ 5h ago         │  │ Yesterday      │ │
│  │ 2h ago [Open]   │  │ [Open]         │  │ [Open]         │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────┘ │
│  ┌─────────────────┐  ┌─────────────────┐                       │
│  │ Task Manager    │  │ + New Project  │                       │
│  │ 3 days ago      │  │                │                       │
│  │ [Open]          │  │                │                       │
│  └─────────────────┘  └─────────────────┘                       │
└────────────────────────────────────────────────────────────────┘
```

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

- **Framework**: React 18 + TypeScript + Tailwind CSS
- **Drag & Drop**: @dnd-kit/core for form/page builders
- **Flow Editor**: React Flow for BPMN workflow builder
- **Canvas Rendering**: SVG + HTML for ERD in data modeler
- **Icons**: Lucide React (consistent icon set)
- **Animations**: Tailwind transitions (150-300ms)
- **Accessibility**: ARIA labels, keyboard navigation, focus traps
- **Auto-save**: Debounced at 30 seconds after last change
- **Toast Notifications**: Sonner for success/error/info messages
- **Modals**: Radix UI Dialog for accessibility
- **Tooltips**: Radix UI Tooltip for contextual help

---

**Document Control**

| Version | Date | Author | Changes |
|---------|------|--------|---------|
| 1.0 | 2026-10-15 | UX Designer | Initial Draft - Complete UI/UX wireframes & mockups for No-Code IDE platform |
| 1.1 | 2026-10-16 | Engineering | Add Web IDE auth + project dashboard, new List Builder wireframe; note web flows |