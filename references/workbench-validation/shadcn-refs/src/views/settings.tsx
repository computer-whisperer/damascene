import {
  Braces,
  Keyboard,
  MonitorCog,
  Network,
  Palette,
  RotateCcw,
  Search,
  SlidersHorizontal,
  SquareCode,
  Wrench,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldLabel,
} from "@/components/ui/field"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
  InputGroupText,
} from "@/components/ui/input-group"
import { Kbd } from "@/components/ui/kbd"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"

/* ------------------------------------------------------------------ data */

const CATEGORIES = [
  { id: "general", label: "General", icon: SlidersHorizontal },
  { id: "appearance", label: "Appearance", icon: Palette, dirty: 3 },
  { id: "editor", label: "Editor", icon: SquareCode },
  { id: "keyboard", label: "Keyboard", icon: Keyboard },
  { id: "network", label: "Network", icon: Network },
  { id: "advanced", label: "Advanced", icon: Wrench },
] as const

const ACCENTS = [
  { id: "cobalt", label: "Cobalt", hex: "#3b82f6" },
  { id: "teal", label: "Teal", hex: "#14b8a6" },
  { id: "violet", label: "Violet", hex: "#8b5cf6" },
  { id: "amber", label: "Amber", hex: "#f59e0b" },
  { id: "rose", label: "Rose", hex: "#f43f5e" },
  { id: "graphite", label: "Graphite", hex: "#94a3b8" },
]

/* ------------------------------------------------------- row primitives */

/** Settings row: label + dim one-line description, control right-aligned. */
function SettingRow({
  htmlFor,
  label,
  description,
  children,
}: {
  htmlFor?: string
  label: string
  description: string
  children: React.ReactNode
}) {
  return (
    <Field orientation="horizontal" className="gap-6 px-4 py-2">
      <FieldContent className="gap-0.5">
        <FieldLabel htmlFor={htmlFor} className="text-[13px] font-medium">
          {label}
        </FieldLabel>
        <FieldDescription className="text-xs">{description}</FieldDescription>
      </FieldContent>
      <div className="flex w-[248px] shrink-0 justify-end self-center">
        {children}
      </div>
    </Field>
  )
}

/** Settings row with a leading checkbox, shadcn's canonical field layout. */
function CheckRow({
  id,
  label,
  description,
  defaultChecked,
}: {
  id: string
  label: string
  description: string
  defaultChecked?: boolean
}) {
  return (
    <Field orientation="horizontal" className="gap-3 px-4 py-2">
      <Checkbox id={id} defaultChecked={defaultChecked} className="mt-0.5" />
      <FieldContent className="gap-0.5">
        <FieldLabel htmlFor={id} className="text-[13px] font-medium">
          {label}
        </FieldLabel>
        <FieldDescription className="text-xs">{description}</FieldDescription>
      </FieldContent>
    </Field>
  )
}

/** Settings row with a trailing switch. */
function SwitchRow({
  id,
  label,
  description,
  defaultChecked,
}: {
  id: string
  label: string
  description: string
  defaultChecked?: boolean
}) {
  return (
    <Field orientation="horizontal" className="gap-6 px-4 py-2">
      <FieldContent className="gap-0.5">
        <FieldLabel htmlFor={id} className="text-[13px] font-medium">
          {label}
        </FieldLabel>
        <FieldDescription className="text-xs">{description}</FieldDescription>
      </FieldContent>
      <Switch id={id} defaultChecked={defaultChecked} className="self-center" />
    </Field>
  )
}

function GroupLabel({ children }: { children: React.ReactNode }) {
  return (
    <h3 className="mb-2 px-1 text-[11px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
      {children}
    </h3>
  )
}

function Group({ children }: { children: React.ReactNode }) {
  return (
    <div className="divide-y divide-border overflow-hidden rounded-lg border bg-card/40">
      {children}
    </div>
  )
}

/* -------------------------------------------------------------- the view */

export default function View() {
  return (
    <TooltipProvider delayDuration={200}>
      <div className="flex h-[800px] w-[1280px] flex-col bg-background font-sans text-foreground antialiased">
        {/* ---------------------------------------------------- header */}
        <header className="flex h-12 shrink-0 items-center gap-3 border-b bg-card/40 pr-3 pl-4">
          <MonitorCog className="size-4 text-muted-foreground" />
          <h1 className="text-sm font-semibold tracking-tight">Preferences</h1>
          <Badge
            variant="outline"
            className="h-5 rounded-sm border-border/70 px-1.5 text-[10px] font-medium tracking-wide text-muted-foreground uppercase"
          >
            User
          </Badge>

          <div className="ml-auto flex items-center gap-2">
            <InputGroup className="h-8 w-[300px] bg-input/20">
              <InputGroupAddon>
                <Search className="text-muted-foreground" />
              </InputGroupAddon>
              <InputGroupInput
                placeholder="Search settings"
                className="text-[13px] md:text-[13px]"
              />
              <InputGroupAddon align="inline-end">
                <Kbd className="bg-muted/70">⌘K</Kbd>
              </InputGroupAddon>
            </InputGroup>

            <Separator orientation="vertical" className="!h-5" />

            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label="Open settings.json"
                  className="text-muted-foreground"
                >
                  <Braces />
                </Button>
              </TooltipTrigger>
              <TooltipContent side="bottom">Open settings.json</TooltipContent>
            </Tooltip>
          </div>
        </header>

        <div className="flex min-h-0 flex-1">
          {/* ------------------------------------------------ sidebar */}
          <aside className="flex w-[220px] shrink-0 flex-col border-r bg-card/25">
            <nav className="flex flex-col gap-0.5 p-2">
              <h2 className="mb-1 px-2 pt-1 text-[11px] font-semibold tracking-[0.08em] text-muted-foreground uppercase">
                Categories
              </h2>
              {CATEGORIES.map(({ id, label, icon: Icon, ...rest }) => {
                const active = id === "appearance"
                const dirty = "dirty" in rest ? rest.dirty : undefined
                return (
                  <Button
                    key={id}
                    variant={active ? "secondary" : "ghost"}
                    size="sm"
                    aria-current={active ? "page" : undefined}
                    className={
                      active
                        ? "relative h-8 w-full justify-start gap-2.5 px-2.5 text-[13px] font-medium before:absolute before:top-1.5 before:bottom-1.5 before:-left-1 before:w-[2px] before:rounded-full before:bg-foreground/70"
                        : "h-8 w-full justify-start gap-2.5 px-2.5 text-[13px] font-normal text-muted-foreground hover:text-foreground"
                    }
                  >
                    <Icon className="opacity-80" />
                    {label}
                    {dirty ? (
                      <span className="ml-auto size-1.5 rounded-full bg-foreground/50" />
                    ) : null}
                  </Button>
                )
              })}
            </nav>

            <div className="mt-auto border-t px-4 py-3">
              <p className="text-[11px] text-muted-foreground">
                Damascene Studio
              </p>
              <p className="text-[11px] tabular-nums text-muted-foreground/70">
                Version 2.4.1 · stable
              </p>
            </div>
          </aside>

          {/* ------------------------------------------------ content */}
          <main className="flex min-w-0 flex-1 flex-col">
            <div className="min-h-0 flex-1 overflow-hidden px-10 pt-5">
              <div className="mx-auto w-full max-w-[740px]">
                <div className="mb-5">
                  <h2 className="text-[15px] font-semibold tracking-tight">
                    Appearance
                  </h2>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    Theme, typography, and motion for the workbench shell.
                    Changes apply to all windows.
                  </p>
                </div>

                <GroupLabel>Theme &amp; typography</GroupLabel>
                <Group>
                  <SettingRow
                    htmlFor="theme"
                    label="Theme"
                    description="Color theme used across editors, panels, and dialogs."
                  >
                    <Select defaultValue="obsidian">
                      <SelectTrigger id="theme" size="sm" className="w-[220px]">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="obsidian">Obsidian Dark</SelectItem>
                        <SelectItem value="graphite">Graphite Dark</SelectItem>
                        <SelectItem value="nord">Nord Deep</SelectItem>
                        <SelectItem value="solarized">
                          Solarized Light
                        </SelectItem>
                        <SelectSeparator />
                        <SelectItem value="system">Follow system</SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>

                  <SettingRow
                    htmlFor="ui-font"
                    label="UI font family"
                    description="Applies to menus, sidebars, and panel labels."
                  >
                    <Select defaultValue="inter">
                      <SelectTrigger
                        id="ui-font"
                        size="sm"
                        className="w-[220px]"
                      >
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="inter">Inter</SelectItem>
                        <SelectItem value="ibm-plex">IBM Plex Sans</SelectItem>
                        <SelectItem value="source">Source Sans 3</SelectItem>
                        <SelectItem value="segoe">Segoe UI</SelectItem>
                        <SelectSeparator />
                        <SelectItem value="system-ui">
                          System default
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </SettingRow>

                  <SettingRow
                    htmlFor="font-size"
                    label="Font size"
                    description="Base size for interface text. Editor size is set separately."
                  >
                    <InputGroup className="h-8 w-[104px]">
                      <InputGroupInput
                        id="font-size"
                        inputMode="numeric"
                        defaultValue="13"
                        className="text-[13px] tabular-nums md:text-[13px]"
                      />
                      <InputGroupAddon align="inline-end">
                        <InputGroupText className="text-xs">px</InputGroupText>
                      </InputGroupAddon>
                    </InputGroup>
                  </SettingRow>

                  <SettingRow
                    label="Accent color"
                    description="Used for selection, focus rings, and active indicators."
                  >
                    <div className="flex items-center gap-3">
                      <span className="text-xs text-muted-foreground">
                        Cobalt
                      </span>
                      <ToggleGroup
                        type="single"
                        defaultValue="cobalt"
                        spacing={1.5}
                        variant="outline"
                        size="sm"
                        aria-label="Accent color"
                      >
                        {/* NB: no Tooltip on these items — TooltipTrigger
                            asChild overwrites the toggle's data-state, which
                            kills the data-[state=on] selected styling. */}
                        {ACCENTS.map((accent) => (
                          <ToggleGroupItem
                            key={accent.id}
                            value={accent.id}
                            aria-label={accent.label}
                            title={accent.label}
                            className="size-7 min-w-0 rounded-md border-transparent p-0 shadow-none hover:bg-accent/50 data-[state=on]:border-foreground/50 data-[state=on]:bg-accent"
                          >
                            <span
                              className="size-3.5 rounded-full"
                              style={{ backgroundColor: accent.hex }}
                            />
                          </ToggleGroupItem>
                        ))}
                      </ToggleGroup>
                    </div>
                  </SettingRow>
                </Group>

                <div className="h-5" />

                <GroupLabel>Motion &amp; chrome</GroupLabel>
                <Group>
                  <CheckRow
                    id="smooth-scrolling"
                    label="Smooth scrolling"
                    description="Animate scroll position instead of jumping by line."
                    defaultChecked
                  />
                  <CheckRow
                    id="reduce-motion"
                    label="Reduce motion"
                    description="Disable panel transitions, easing, and decorative animation."
                  />
                  <CheckRow
                    id="status-bar"
                    label="Show status bar"
                    description="Keep the bottom bar with branch, diagnostics, and cursor position."
                    defaultChecked
                  />
                </Group>

                <div className="h-5" />

                <GroupLabel>Window</GroupLabel>
                <Group>
                  <SwitchRow
                    id="compact-mode"
                    label="Compact mode"
                    description="Tighten row heights and paddings throughout the workbench."
                    defaultChecked
                  />
                  <SwitchRow
                    id="translucency"
                    label="Window translucency"
                    description="Blur the desktop behind panels. May reduce frame rate."
                  />
                </Group>
              </div>
            </div>

            {/* ------------------------------------------- action bar */}
            <div className="shrink-0 border-t bg-card/25 px-10">
              <div className="mx-auto flex h-14 w-full max-w-[740px] items-center gap-3">
                <Button
                  variant="ghost"
                  size="sm"
                  className="-ml-3 gap-2 text-[13px] text-muted-foreground hover:text-foreground"
                >
                  <RotateCcw />
                  Restore defaults
                </Button>
                <span className="text-xs text-muted-foreground/70">
                  3 settings modified
                </span>
                <div className="ml-auto flex items-center gap-2">
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-[13px] text-muted-foreground hover:text-foreground"
                  >
                    Cancel
                  </Button>
                  <Button size="sm" className="px-4 text-[13px]">
                    Save
                  </Button>
                </div>
              </div>
            </div>
          </main>
        </div>
      </div>
    </TooltipProvider>
  )
}
