import {
  AlertTriangle,
  BarChart3,
  Bell,
  Check,
  ChevronRight,
  CircleDollarSign,
  CreditCard,
  FileText,
  GitBranch,
  KeyRound,
  LayoutDashboard,
  Laptop,
  Mail,
  MoreHorizontal,
  Palette,
  PanelLeft,
  RefreshCw,
  Search,
  Settings,
  Shield,
  ShoppingCart,
  SlidersHorizontal,
  TrendingUp,
  Users,
} from "lucide-react"
import type React from "react"
import { cn } from "./lib/utils"

const referenceUiScale = Number(new URLSearchParams(window.location.search).get("uiScale") ?? "1")
if (Number.isFinite(referenceUiScale) && referenceUiScale > 0) {
  document.documentElement.style.fontSize = `${16 * referenceUiScale}px`
  document.documentElement.dataset.referenceUiScale = String(referenceUiScale)
}

const densityName = new URLSearchParams(window.location.search).get("density") ?? "comfortable"
const density = {
  compact: {
    button: "h-8 px-3 py-1.5",
    badge: "h-5 min-w-14 px-2",
    cardTitle: "p-4 pb-2",
    cardBody: "p-4 pt-1",
    cardP: "p-3",
    input: "h-8 px-2.5",
    iconBox: "h-6 w-6",
    pagePad: "p-5",
    sectionPad: "p-3",
    gap: "gap-3",
    space: "space-y-3",
    navRow: "h-8",
    tableHeader: "h-8",
    tableRow: "h-10",
    preferenceRow: "px-3 py-2",
  },
  comfortable: {
    button: "h-9 px-4 py-2",
    badge: "h-6 min-w-[4.5rem] px-2.5",
    cardTitle: "p-5 pb-3",
    cardBody: "p-5 pt-2",
    cardP: "p-4",
    input: "h-9 px-3",
    iconBox: "h-7 w-7",
    pagePad: "p-7",
    sectionPad: "p-4",
    gap: "gap-4",
    space: "space-y-4",
    navRow: "h-10",
    tableHeader: "h-9",
    tableRow: "h-[52px]",
    preferenceRow: "px-4 py-3",
  },
  spacious: {
    button: "h-10 px-5 py-2.5",
    badge: "h-7 min-w-20 px-3",
    cardTitle: "p-6 pb-4",
    cardBody: "p-6 pt-2",
    cardP: "p-5",
    input: "h-11 px-4",
    iconBox: "h-8 w-8",
    pagePad: "p-8",
    sectionPad: "p-5",
    gap: "gap-5",
    space: "space-y-5",
    navRow: "h-11",
    tableHeader: "h-10",
    tableRow: "h-14",
    preferenceRow: "px-5 py-4",
  },
}[densityName as "compact" | "comfortable" | "spacious"] ?? {
  button: "h-9 px-4 py-2",
  badge: "h-6 min-w-[4.5rem] px-2.5",
  cardTitle: "p-5 pb-3",
  cardBody: "p-5 pt-2",
  cardP: "p-4",
  input: "h-10 px-3",
  iconBox: "h-7 w-7",
  pagePad: "p-7",
  sectionPad: "p-4",
  gap: "gap-4",
  space: "space-y-4",
  navRow: "h-10",
  tableHeader: "h-9",
  tableRow: "h-[52px]",
  preferenceRow: "px-4 py-3",
}

const measure = (id: string) => ({ "data-calibration-id": id })

function Button({
  className,
  variant = "default",
  measureId,
  children,
}: {
  className?: string
  variant?: "default" | "secondary" | "outline" | "ghost" | "destructive"
  measureId?: string
  children: React.ReactNode
}) {
  const variants = {
    default: "bg-primary text-primary-foreground shadow hover:bg-primary/90",
    secondary: "bg-secondary text-secondary-foreground shadow-sm hover:bg-secondary/80",
    outline: "border border-input bg-background shadow-sm hover:bg-accent hover:text-accent-foreground",
    ghost: "hover:bg-accent hover:text-accent-foreground",
    destructive:
      "bg-destructive text-destructive-foreground shadow-sm hover:bg-destructive/90",
  }
  return (
    <button
      {...(measureId ? measure(measureId) : {})}
      className={cn(
        "inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-50",
        density.button,
        variants[variant],
        className,
      )}
    >
      {children}
    </button>
  )
}

function Badge({
  children,
  tone = "default",
  measureId,
}: {
  children: React.ReactNode
  tone?: "default" | "success" | "warning" | "destructive" | "info"
  measureId?: string
}) {
  const tones = {
    default: "border-transparent bg-secondary text-secondary-foreground",
    success: "border-emerald-500/40 bg-emerald-500/10 text-emerald-400",
    warning: "border-amber-500/40 bg-amber-500/10 text-amber-300",
    destructive: "border-red-500/40 bg-red-500/10 text-red-300",
    info: "border-sky-500/40 bg-sky-500/10 text-sky-300",
  }
  return (
    <div
      {...(measureId ? measure(measureId) : {})}
      className={cn(
        "inline-flex w-fit items-center justify-center rounded-full border text-xs font-semibold",
        density.badge,
        tones[tone],
      )}
    >
      {children}
    </div>
  )
}

function Card({
  title,
  children,
  className,
  measureId,
}: {
  title: string
  children: React.ReactNode
  className?: string
  measureId?: string
}) {
  return (
    <section
      data-calibration-boundary
      {...(measureId ? measure(measureId) : {})}
      className={cn("rounded-xl border bg-card text-card-foreground shadow-sm", className)}
    >
      <div className={cn("space-y-1.5", density.cardTitle)}>
        <h3 className="text-base font-semibold leading-none tracking-tight">{title}</h3>
      </div>
      <div className={density.cardBody}>{children}</div>
    </section>
  )
}

function Input({ value, invalid, measureId }: { value: string; invalid?: boolean; measureId?: string }) {
  return (
    <div
      {...(measureId ? measure(measureId) : {})}
      className={cn(
        "flex w-full items-center rounded-md border border-input bg-background text-sm shadow-sm",
        density.input,
        invalid && "border-red-500 text-red-100 ring-1 ring-red-500/20",
      )}
    >
      {value}
    </div>
  )
}

function IconBox({ children }: { children: React.ReactNode }) {
  return (
    <div className={cn("flex items-center justify-center rounded-md border bg-muted text-muted-foreground", density.iconBox)}>
      {children}
    </div>
  )
}

const rows = [
  ["OK", "Settings card", "core", "selected", "success"],
  ["WARN", "Command palette density", "widgets", "needs work", "warning"],
  ["ERR", "Disabled and invalid states", "style", "missing", "destructive"],
  ["INFO", "Token resolution", "theme", "planned", "info"],
  ["OK", "Popover elevation", "shader", "queued", "success"],
] as const

export function App() {
  const view = new URLSearchParams(window.location.search).get("view")
  if (view === "dashboard-01") {
    return <DashboardReference />
  }
  if (view === "settings-01") {
    return <SettingsReference />
  }
  return <CalibrationReference />
}

function CalibrationReference() {
  return (
    <main {...measure("root")} className="flex min-h-screen bg-background text-foreground">
      <aside {...measure("sidebar")} className={cn("flex w-[220px] flex-col border-r bg-card", density.cardTitle)}>
        <div {...measure("sidebar.brand")}>
          <h2 className="text-2xl font-bold">Damascene</h2>
          <p className="mt-2 text-sm text-muted-foreground">calibration</p>
        </div>
        <nav className="mt-10 space-y-2">
          {["Overview", "Commands", "Tables", "Forms"].map((item, i) => (
            <div
              key={item}
              {...(i === 0 ? measure("sidebar.nav.row") : {})}
              className={cn(
                "flex items-center gap-3 rounded-lg px-2 text-sm font-medium",
                density.navRow,
                i === 0 && "border border-sky-500/50 bg-sky-500/10",
              )}
            >
              <IconBox>{String(i + 1).padStart(2, "0")}</IconBox>
              {item}
            </div>
          ))}
        </nav>
        <div className="mt-auto">
          <Badge>dark theme</Badge>
        </div>
      </aside>

      <section className={cn("flex flex-1 flex-col", density.space, density.pagePad)}>
        <header {...measure("header")} className="flex h-14 items-start gap-4">
          <div>
            <h1 {...measure("page.title")} className="text-3xl font-bold tracking-tight">Polish calibration</h1>
            <p {...measure("page.subtitle")} className="mt-2 text-sm text-muted-foreground">
              A representative app surface for default tuning.
            </p>
          </div>
          <div className="ml-auto flex gap-2">
            <Button variant="outline" measureId="action.secondary">Preview</Button>
            <Button measureId="action.primary">Publish</Button>
          </div>
        </header>

        <div className={cn("grid grid-cols-3", density.gap)}>
          <Kpi title="Latency" value="42 ms" delta="-18%" tone="success" measureId="kpi" />
          <Kpi title="Runs" value="1,284" delta="+12%" tone="success" />
          <Kpi title="Errors" value="7" delta="+2" tone="destructive" />
        </div>

        <div className={cn("grid min-h-0 flex-1 grid-cols-[minmax(560px,1fr)_320px]", density.gap)}>
          <Card title="Reference rows" className="min-h-0" measureId="table.card">
            <div {...measure("table.header")} className={cn("grid grid-cols-[7rem_1fr_4.5rem_5.5rem] items-center gap-3 px-2 text-sm text-muted-foreground", density.tableHeader)}>
              <span>Status</span>
              <span>Surface</span>
              <span>Owner</span>
              <span>State</span>
            </div>
            <div className="my-2 border-t" />
            <div className="space-y-1">
              {rows.map(([status, title, owner, state, tone], i) => (
                <div
                  key={title}
                  {...(i === 0 ? measure("table.row") : {})}
                  className={cn(
                    "grid grid-cols-[7rem_1fr_4.5rem_5.5rem] items-center gap-3 rounded-md px-2 text-sm",
                    density.tableRow,
                    i === 0 && "border border-sky-500/50 bg-sky-500/10",
                  )}
                >
                  <Badge tone={tone} measureId={i === 0 ? "table.badge" : undefined}>{status}</Badge>
                  <div>
                    <div className="truncate font-medium">{title}</div>
                    <div className="truncate text-xs text-muted-foreground">Default styling probe.</div>
                  </div>
                  <div className="truncate text-muted-foreground">{owner}</div>
                  <div className="truncate">{state}</div>
                </div>
              ))}
            </div>
          </Card>

          <Card title="Command surface" measureId="command.card">
            <div className="relative">
              <Search className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
              <div {...measure("command.input")} className={cn("flex items-center rounded-md border bg-background pl-9 text-sm text-muted-foreground shadow-sm", density.input)}>
                Search commands...
              </div>
            </div>
            <div className="mt-4 rounded-lg border bg-card p-2 shadow-md">
              <CommandRow icon={<GitBranch />} label="New branch" shortcut="Ctrl+B" measureId="command.row" />
              <CommandRow icon={<Check />} label="Commit staged files" shortcut="Ctrl+Enter" />
              <CommandRow icon={<RefreshCw />} label="Refresh repository" shortcut="Ctrl+R" />
              <CommandRow icon={<AlertTriangle />} label="Force push" shortcut="Danger" />
            </div>
            <div className={cn("mt-4 rounded-lg border bg-muted/50", density.sectionPad)}>
              <h3 className="font-semibold">Form state probes</h3>
              <div className="mt-4 space-y-3">
                <Input value="Valid input" measureId="form.input" />
                <Input value="Invalid input" invalid />
                <div className="flex gap-2">
                  <Button variant="secondary" className="opacity-50">
                    Disabled
                  </Button>
                  <Button>Loading</Button>
                </div>
              </div>
              <p className="mt-4 text-sm text-muted-foreground">
                These are currently hand-styled probes; they should become semantic modifiers.
              </p>
            </div>
          </Card>
        </div>
      </section>
    </main>
  )
}

const dashboardRows = [
  ["Cover page", "Cover page", "In Process", "18", "5", "Eddie Lake"],
  ["Table of contents", "Table of contents", "Done", "29", "24", "Eddie Lake"],
  ["Executive summary", "Narrative", "Done", "10", "13", "Iris Joe"],
  ["Technical approach", "Narrative", "Done", "27", "23", "Tyler Davis"],
  ["Design notes", "Narrative", "In Process", "2", "16", "Maya Stone"],
  ["Appendix", "Appendix", "Blocked", "13", "8", "Noah Kim"],
] as const

function DashboardReference() {
  return (
    <main {...measure("root")} className="flex h-screen overflow-hidden bg-background text-foreground">
      <aside {...measure("sidebar")} className="flex w-[244px] flex-col border-r bg-card">
        <div className="flex h-14 items-center gap-2 border-b px-4">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <LayoutDashboard className="h-4 w-4" />
          </div>
          <div>
            <div className="text-sm font-semibold">Acme Inc.</div>
            <div className="text-xs text-muted-foreground">Enterprise</div>
          </div>
        </div>
        <nav className="space-y-6 p-3 text-sm">
          <SidebarSection
            title="Platform"
            items={[
              ["Dashboard", <LayoutDashboard className="h-4 w-4" />, true],
              ["Lifecycle", <RefreshCw className="h-4 w-4" />, false],
              ["Analytics", <BarChart3 className="h-4 w-4" />, false],
              ["Projects", <FileText className="h-4 w-4" />, false],
            ]}
          />
          <SidebarSection
            title="Documents"
            items={[
              ["Data library", <FileText className="h-4 w-4" />, false],
              ["Reports", <TrendingUp className="h-4 w-4" />, false],
              ["Team", <Users className="h-4 w-4" />, false],
            ]}
          />
        </nav>
        <div className="mt-auto border-t p-3">
          <div className="flex items-center gap-3 rounded-lg px-2 py-2">
            <div className="flex h-8 w-8 items-center justify-center rounded-full bg-muted text-xs font-semibold">
              AK
            </div>
            <div className="min-w-0">
              <div className="truncate text-sm font-medium">Alicia Koch</div>
              <div className="truncate text-xs text-muted-foreground">alicia@example.com</div>
            </div>
          </div>
        </div>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <header {...measure("header")} className="flex h-14 items-center gap-3 border-b px-4">
          <Button variant="ghost" className="h-8 w-8 px-0">
            <PanelLeft className="h-4 w-4" />
          </Button>
          <div className="h-5 border-l" />
          <h1 {...measure("page.title")} className="text-base font-semibold">Documents</h1>
          <div className="ml-auto flex items-center gap-2">
            <div className="relative w-[260px]">
              <Search className="absolute left-2.5 top-2.5 h-4 w-4 text-muted-foreground" />
              <div {...measure("command.input")} className="flex h-9 items-center rounded-md border bg-background pl-8 text-sm text-muted-foreground">
                Search...
              </div>
            </div>
            <Button variant="ghost" className="h-8 w-8 px-0">
              <Bell className="h-4 w-4" />
            </Button>
          </div>
        </header>

        <div className={cn("min-h-0 flex-1 overflow-hidden", density.space, density.sectionPad)}>
          <div className={cn("grid grid-cols-4", density.gap)}>
              <MetricCard
                icon={<CircleDollarSign className="h-4 w-4" />}
                title="Total Revenue"
                value="$1,250.00"
                delta="+12.5%"
                note="Trending up this month"
                measureId="kpi"
              />
            <MetricCard
              icon={<Users className="h-4 w-4" />}
              title="New Customers"
              value="1,234"
              delta="-20%"
              note="Acquisition needs attention"
            />
            <MetricCard
              icon={<ShoppingCart className="h-4 w-4" />}
              title="Active Accounts"
              value="45,678"
              delta="+12.5%"
              note="Strong user retention"
            />
            <MetricCard
              icon={<TrendingUp className="h-4 w-4" />}
              title="Growth Rate"
              value="4.5%"
              delta="+4.5%"
              note="Meets growth projections"
            />
          </div>

          <div className={cn("grid grid-cols-[minmax(0,1fr)_330px]", density.gap)}>
            <section data-calibration-boundary {...measure("chart.card")} className={cn("rounded-xl border bg-card shadow-sm", density.cardP)}>
              <div className="flex items-center gap-2">
                <div>
                  <h2 className="text-base font-semibold">Visitors for the last 6 months</h2>
                  <p className="text-sm text-muted-foreground">Total visitors by channel.</p>
                </div>
                <Button variant="outline" className="ml-auto h-8 px-3">
                  Last 6 months
                </Button>
              </div>
              <div className="mt-5 flex h-[148px] items-end gap-2">
                {[48, 72, 56, 90, 64, 80, 108, 84, 122, 96, 136, 118, 158, 126, 148, 172, 140, 184].map(
                  (height, i) => (
                    <div key={i} className="flex flex-1 items-end gap-1">
                      <div
                        className="w-full rounded-t bg-primary/80"
                        style={{ height }}
                      />
                      <div
                        className="w-full rounded-t bg-muted"
                        style={{ height: Math.max(24, height - 34) }}
                      />
                    </div>
                  ),
                )}
              </div>
            </section>

            <section data-calibration-boundary {...measure("sales.card")} className={cn("rounded-xl border bg-card shadow-sm", density.cardP)}>
              <h2 className="text-base font-semibold">Recent Sales</h2>
              <p className="text-sm text-muted-foreground">You made 265 sales this month.</p>
              <div className="mt-5 space-y-4">
                <Sale name="Olivia Martin" email="olivia@example.com" amount="+$1,999.00" />
                <Sale name="Jackson Lee" email="jackson@example.com" amount="+$39.00" />
                <Sale name="Isabella Nguyen" email="isabella@example.com" amount="+$299.00" />
                <Sale name="William Kim" email="will@example.com" amount="+$99.00" />
              </div>
            </section>
          </div>

          <section
            data-calibration-boundary
            {...measure("table.card")}
            className="overflow-hidden rounded-xl border bg-card shadow-sm"
          >
            <div className={cn("flex items-center gap-3 border-b px-4", density.navRow)}>
              <h2 className="text-base font-semibold">Documents</h2>
              <Button variant="outline" className="ml-auto h-8 px-3">
                Columns
              </Button>
            </div>
            <div {...measure("table.header")} className={cn("grid grid-cols-[2.2rem_1.8fr_1fr_6.5rem_4rem_4rem_8rem_2rem] items-center gap-3 border-b px-4 text-xs font-medium text-muted-foreground", density.tableHeader)}>
              <span />
              <span>Header</span>
              <span>Section Type</span>
              <span>Status</span>
              <span>Target</span>
              <span>Limit</span>
              <span>Reviewer</span>
              <span />
            </div>
            <div>
              {dashboardRows.slice(0, 2).map(([header, section, status, target, limit, reviewer], i) => (
                <div
                  key={header}
                  {...(i === 0 ? measure("table.row") : {})}
                  className={cn("grid grid-cols-[2.2rem_1.8fr_1fr_6.5rem_4rem_4rem_8rem_2rem] items-center gap-3 border-b px-4 text-sm last:border-b-0", density.tableRow)}
                >
                  <span className="text-muted-foreground">::</span>
                  <button className="truncate text-left font-medium underline-offset-4 hover:underline">
                    {header}
                  </button>
                  <span className="truncate text-muted-foreground">{section}</span>
                  <StatusBadge status={status} />
                  <span>{target}</span>
                  <span>{limit}</span>
                  <span className="truncate text-muted-foreground">{reviewer}</span>
                  <MoreHorizontal className="h-4 w-4 text-muted-foreground" />
                </div>
              ))}
            </div>
          </section>
        </div>
      </section>
    </main>
  )
}

function SidebarSection({
  title,
  items,
}: {
  title: string
  items: readonly [string, React.ReactNode, boolean][]
}) {
  return (
    <div>
      <div className="mb-2 px-2 text-xs font-medium text-muted-foreground">{title}</div>
      <div className="space-y-1">
        {items.map(([label, icon, active]) => (
          <div
            key={label}
            {...(active ? measure("sidebar.nav.row") : {})}
            className={cn(
              "flex h-8 items-center gap-2 rounded-md px-2 text-sm font-medium",
              active ? "bg-muted text-foreground" : "text-muted-foreground hover:bg-muted/70",
            )}
          >
            {icon}
            <span>{label}</span>
            {active && <ChevronMarker />}
          </div>
        ))}
      </div>
    </div>
  )
}

function ChevronMarker() {
  return <ChevronRight className="ml-auto h-4 w-4 text-muted-foreground" />
}

function MetricCard({
  icon,
  title,
  value,
  delta,
  note,
  measureId,
}: {
  icon: React.ReactNode
  title: string
  value: string
  delta: string
  note: string
  measureId?: string
}) {
  return (
    <section data-calibration-boundary {...(measureId ? measure(`${measureId}.card`) : {})} className={cn("rounded-xl border bg-card shadow-sm", density.cardP)}>
      <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2">
        <div className="flex min-w-0 items-center gap-2 text-sm text-muted-foreground">
          {icon}
          <span className="truncate">{title}</span>
        </div>
        <Badge tone={delta.startsWith("+") ? "success" : "warning"} measureId={measureId ? `${measureId}.badge` : undefined}>{delta}</Badge>
      </div>
      <div {...(measureId ? measure(`${measureId}.value`) : {})} className="mt-3 truncate text-2xl font-semibold tracking-tight">{value}</div>
      <p className="mt-3 truncate text-xs text-muted-foreground">{note}</p>
    </section>
  )
}

function Sale({ name, email, amount }: { name: string; email: string; amount: string }) {
  return (
    <div className="flex items-center gap-3">
      <div className="flex h-9 w-9 items-center justify-center rounded-full bg-muted text-xs font-semibold">
        {name
          .split(" ")
          .map((part) => part[0])
          .join("")}
      </div>
      <div className="min-w-0">
        <div className="truncate text-sm font-medium">{name}</div>
        <div className="truncate text-xs text-muted-foreground">{email}</div>
      </div>
      <div className="ml-auto text-sm font-medium">{amount}</div>
    </div>
  )
}

function StatusBadge({ status }: { status: string }) {
  if (status === "Done") {
    return <Badge tone="success">Done</Badge>
  }
  if (status === "Blocked") {
    return <Badge tone="destructive">Blocked</Badge>
  }
  return <Badge tone="info">In Process</Badge>
}

function SettingsReference() {
  return (
    <main {...measure("root")} className="flex h-screen overflow-hidden bg-background text-foreground">
      <aside {...measure("sidebar")} className="flex w-[244px] flex-col border-r bg-card">
        <div className="flex h-14 items-center gap-2 border-b px-4">
          <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary text-primary-foreground">
            <Settings className="h-4 w-4" />
          </div>
          <div>
            <div className="text-sm font-semibold">Workspace</div>
            <div className="text-xs text-muted-foreground">Settings</div>
          </div>
        </div>
        <nav className="space-y-6 p-3 text-sm">
          <SidebarSection
            title="Personal"
            items={[
              ["Profile", <Users className="h-4 w-4" />, false],
              ["Account", <Settings className="h-4 w-4" />, true],
              ["Security", <Shield className="h-4 w-4" />, false],
              ["Notifications", <Bell className="h-4 w-4" />, false],
            ]}
          />
          <SidebarSection
            title="Workspace"
            items={[
              ["Billing", <CreditCard className="h-4 w-4" />, false],
              ["Appearance", <Palette className="h-4 w-4" />, false],
              ["Integrations", <SlidersHorizontal className="h-4 w-4" />, false],
            ]}
          />
        </nav>
        <div className="mt-auto border-t p-3">
          <div className="rounded-lg bg-muted/50 px-3 py-2 text-xs text-muted-foreground">
            Changes sync after save.
          </div>
        </div>
      </aside>

      <section className="flex min-w-0 flex-1 flex-col">
        <header {...measure("header")} className="flex h-14 items-center gap-3 border-b px-4">
          <Button variant="ghost" className="h-8 w-8 px-0">
            <PanelLeft className="h-4 w-4" />
          </Button>
          <div className="h-5 border-l" />
          <h1 {...measure("page.title")} className="text-base font-semibold">Settings</h1>
          <div className="ml-auto flex items-center gap-2">
            <Button variant="outline" className="h-8 px-3">
              Reset
            </Button>
            <Button className="h-8 px-3">Save changes</Button>
          </div>
        </header>

        <div className={cn("grid min-h-0 flex-1 grid-cols-[220px_minmax(0,1fr)_300px] overflow-hidden", density.gap, density.sectionPad)}>
          <section data-calibration-boundary className="rounded-xl border bg-card p-2 shadow-sm">
            <div className="space-y-1">
              {["Account", "Security", "Notifications", "Appearance", "Billing"].map((item, i) => (
                <button
                  key={item}
                  {...(i === 0 ? measure("settings.nav.row") : {})}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-3 text-left text-sm font-medium",
                    density.navRow,
                    i === 0 ? "bg-muted text-foreground" : "text-muted-foreground hover:bg-muted/70",
                  )}
                >
                  <span className="h-1.5 w-1.5 rounded-full bg-current" />
                  {item}
                </button>
              ))}
            </div>
          </section>

          <section className={cn("min-h-0 overflow-hidden", density.space)}>
            <div>
              <h2 {...measure("section.title")} className="text-2xl font-semibold tracking-tight">Account</h2>
              <p {...measure("page.subtitle")} className="mt-1 text-sm text-muted-foreground">
                Manage identity, workspace defaults, and security preferences.
              </p>
            </div>

            <div data-calibration-boundary {...measure("profile.card")} className="rounded-xl border bg-card shadow-sm">
              <div className={cn("border-b", density.cardP)}>
                <h3 className="text-base font-semibold">Profile</h3>
                <p className="mt-1 text-sm text-muted-foreground">
                  This information appears in audit logs and shared documents.
                </p>
              </div>
              <div className={cn("grid grid-cols-2", density.gap, density.cardP)}>
                <SettingField label="Display name" value="Alicia Koch" measureId="form.input" />
                <SettingField label="Email" value="alicia@example.com" icon={<Mail />} />
                <SettingSelect label="Role" value="Workspace admin" />
                <SettingSelect label="Region" value="US East" />
              </div>
            </div>

            <div data-calibration-boundary {...measure("preferences.card")} className="rounded-xl border bg-card shadow-sm">
              <div className={cn("border-b", density.cardP)}>
                <h3 className="text-base font-semibold">Preferences</h3>
                <p className="mt-1 text-sm text-muted-foreground">
                  Defaults used when creating new dashboards and exports.
                </p>
              </div>
              <div className="divide-y">
                <PreferenceRow
                  title="Compact navigation"
                  description="Use tighter rows in the sidebar and command menus."
                  control={<Switch checked />}
                  measureId="preference.row"
                />
                <PreferenceRow
                  title="Email summaries"
                  description="Send a daily digest when documents change."
                  control={<Switch checked={false} />}
                />
                <PreferenceRow
                  title="Require approval"
                  description="Route external sharing through an owner review."
                  control={<Checkbox checked />}
                />
              </div>
            </div>
          </section>

          <aside className={density.space}>
            <section data-calibration-boundary className={cn("rounded-xl border bg-card shadow-sm", density.cardP)}>
              <div className="flex items-center gap-2">
                <IconBox>
                  <KeyRound className="h-4 w-4" />
                </IconBox>
                <h3 className="text-base font-semibold">Security</h3>
              </div>
              <p className="mt-3 text-sm text-muted-foreground">
                Two-factor authentication is enabled for all privileged users.
              </p>
              <div className="mt-4 space-y-3">
                <PreferenceRow title="Passkeys" description="2 registered" control={<Badge tone="success">On</Badge>} compact />
                <PreferenceRow title="Sessions" description="3 active" control={<Button variant="outline" className="h-8 px-3">Review</Button>} compact />
              </div>
            </section>

            <section data-calibration-boundary className={cn("rounded-xl border bg-card shadow-sm", density.cardP)}>
              <div className="flex items-center gap-2">
                <IconBox>
                  <Laptop className="h-4 w-4" />
                </IconBox>
                <h3 className="text-base font-semibold">Interface scale</h3>
              </div>
              <p className="mt-3 text-sm text-muted-foreground">
                Reference captures keep browser zoom fixed and vary root UI scale.
              </p>
              <div className="mt-4">
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                  <span>Dense</span>
                  <span>Default</span>
                </div>
                <div className="mt-2 h-2 rounded-full bg-muted">
                  <div className="h-2 w-2/3 rounded-full bg-primary" />
                </div>
              </div>
            </section>
          </aside>
        </div>
      </section>
    </main>
  )
}

function SettingField({
  label,
  value,
  icon,
  measureId,
}: {
  label: string
  value: string
  icon?: React.ReactNode
  measureId?: string
}) {
  return (
    <label className="block min-w-0">
      <span className="text-sm font-medium">{label}</span>
      <div className="relative mt-2">
        {icon && <span className="absolute left-3 top-2.5 text-muted-foreground [&>svg]:h-4 [&>svg]:w-4">{icon}</span>}
        <div {...(measureId ? measure(measureId) : {})} className={cn("flex items-center rounded-md border bg-background text-sm shadow-sm", density.input, icon && "pl-9")}>
          <span className="truncate">{value}</span>
        </div>
      </div>
    </label>
  )
}

function SettingSelect({ label, value }: { label: string; value: string }) {
  return (
    <label className="block min-w-0">
      <span className="text-sm font-medium">{label}</span>
      <div className={cn("mt-2 flex items-center rounded-md border bg-background text-sm shadow-sm", density.input)}>
        <span className="truncate">{value}</span>
        <ChevronRight className="ml-auto h-4 w-4 rotate-90 text-muted-foreground" />
      </div>
    </label>
  )
}

function PreferenceRow({
  title,
  description,
  control,
  compact = false,
  measureId,
}: {
  title: string
  description: string
  control: React.ReactNode
  compact?: boolean
  measureId?: string
}) {
  return (
    <div {...(measureId ? measure(measureId) : {})} className={cn("flex items-center gap-4", compact ? "py-2" : density.preferenceRow)}>
      <div className="min-w-0">
        <div className="truncate text-sm font-medium">{title}</div>
        <div className="truncate text-xs text-muted-foreground">{description}</div>
      </div>
      <div className="ml-auto shrink-0">{control}</div>
    </div>
  )
}

function Switch({ checked }: { checked: boolean }) {
  return (
    <div
      className={cn(
        "flex h-5 w-9 items-center rounded-full border p-0.5 transition-colors",
        checked ? "border-primary bg-primary" : "border-input bg-input dark:bg-input/80",
      )}
    >
      <div
        className={cn(
          "h-3.5 w-3.5 rounded-full bg-background transition-transform dark:bg-foreground",
          checked && "translate-x-4 dark:bg-primary-foreground",
        )}
      />
    </div>
  )
}

function Checkbox({ checked }: { checked: boolean }) {
  return (
    <div
      className={cn(
        "flex h-4 w-4 items-center justify-center rounded-sm border",
        checked ? "border-primary bg-primary text-primary-foreground" : "border-input",
      )}
    >
      {checked && <Check className="h-3 w-3" />}
    </div>
  )
}

function Kpi({
  title,
  value,
  delta,
  tone,
  measureId,
}: {
  title: string
  value: string
  delta: string
  tone: "success" | "destructive"
  measureId?: string
}) {
  return (
    <Card title={title} measureId={measureId ? `${measureId}.card` : undefined}>
      <div className="flex items-center">
        <div {...(measureId ? measure(`${measureId}.value`) : {})} className="text-3xl font-bold">{value}</div>
        <div className="ml-auto">
          <Badge tone={tone} measureId={measureId ? `${measureId}.badge` : undefined}>{delta}</Badge>
        </div>
      </div>
      <p className="mt-6 text-sm text-muted-foreground">
        {tone === "success" ? "Moving in the expected direction" : "Needs visual attention"}
      </p>
    </Card>
  )
}

function CommandRow({
  icon,
  label,
  shortcut,
  measureId,
}: {
  icon: React.ReactNode
  label: string
  shortcut: string
  measureId?: string
}) {
  return (
    <div {...(measureId ? measure(measureId) : {})} className="flex h-8 items-center gap-3 rounded-md px-2 text-sm hover:bg-accent">
      <span className="shrink-0 text-muted-foreground [&>svg]:h-4 [&>svg]:w-4">{icon}</span>
      <span className="truncate">{label}</span>
      <span className="ml-auto font-mono text-xs text-muted-foreground">{shortcut}</span>
    </div>
  )
}
