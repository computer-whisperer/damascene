import {
  Blocks,
  Camera,
  ChevronDown,
  Clock,
  Cuboid,
  Eye,
  FlipHorizontal2,
  Grid2x2,
  Layers,
  Move3d,
  Printer,
  RotateCcw,
  RotateCw,
  Ruler,
  Save,
  Scaling,
  Settings2,
  Spool,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { ButtonGroup } from "@/components/ui/button-group"
import { Checkbox } from "@/components/ui/checkbox"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
} from "@/components/ui/input-group"
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemMedia,
  ItemTitle,
} from "@/components/ui/item"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import { Label } from "@/components/ui/label"
import {
  Menubar,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarTrigger,
} from "@/components/ui/menubar"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import { Slider } from "@/components/ui/slider"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"

/* ------------------------------------------------------------------ *
 * Meridian Slicer — static 1280x800 mock.
 * ------------------------------------------------------------------ */

const MODEL = "bracket_v4_final.stl"

/** Dense pane header, VS Code sidebar-section style. */
function PaneSection({
  title,
  children,
}: {
  title: string
  children: React.ReactNode
}) {
  return (
    <section className="border-b border-border/70">
      <header className="flex h-[26px] items-center gap-1 border-b border-border/50 bg-secondary/40 px-2 select-none">
        <ChevronDown className="size-3 text-muted-foreground/70" />
        <h2 className="text-[10px] font-semibold tracking-[0.09em] text-muted-foreground uppercase">
          {title}
        </h2>
      </header>
      <div className="flex flex-col py-1">{children}</div>
    </section>
  )
}

/** Label-left / control-right settings row. */
function Row({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <div className="flex h-8 items-center gap-2 px-3">
      <Label className="min-w-0 flex-1 truncate text-xs leading-none font-normal text-muted-foreground">
        {label}
      </Label>
      <div className="flex w-[124px] shrink-0 items-center">{children}</div>
    </div>
  )
}

/** Numeric field, optionally with a trailing unit addon. */
function NumField({ value, unit }: { value: string; unit?: string }) {
  if (!unit) {
    return (
      <Input
        defaultValue={value}
        className="h-7 px-2 font-mono text-xs! tabular-nums"
      />
    )
  }
  return (
    <InputGroup className="h-7">
      <InputGroupInput
        defaultValue={value}
        className="h-7 px-2 font-mono text-xs! tabular-nums"
      />
      <InputGroupAddon
        align="inline-end"
        className="pr-2 text-[11px] font-normal text-muted-foreground"
      >
        {unit}
      </InputGroupAddon>
    </InputGroup>
  )
}

/**
 * Radix's Tooltip.Trigger writes its own `data-state` onto an `asChild`
 * child, which clobbers a ToggleGroupItem's on/off state (silently killing
 * the selected styling). Wrap the item in the trigger box instead.
 */
function ToolTip({
  label,
  side,
  children,
}: {
  label: React.ReactNode
  side: "right" | "bottom"
  children: React.ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span className="flex">{children}</span>
      </TooltipTrigger>
      <TooltipContent side={side} className="flex items-center gap-2">
        {label}
      </TooltipContent>
    </Tooltip>
  )
}

function MiniSelect({
  defaultValue,
  label,
  options,
}: {
  defaultValue: string
  label: string
  options: [string, string][]
}) {
  return (
    <Select defaultValue={defaultValue}>
      <SelectTrigger className="h-7! w-full px-2 text-xs">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          <SelectLabel>{label}</SelectLabel>
          {options.map(([v, t]) => (
            <SelectItem key={v} value={v} className="text-xs">
              {t}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}

const TOOLS: [string, string, typeof Move3d, string][] = [
  ["move", "Move", Move3d, "M"],
  ["rotate", "Rotate", RotateCw, "R"],
  ["scale", "Scale", Scaling, "S"],
  ["mirror", "Mirror", FlipHorizontal2, "H"],
  ["measure", "Measure", Ruler, "E"],
  ["camera", "Camera", Camera, "C"],
]

const MENUS: [string, ([string, string] | "-")[]][] = [
  [
    "File",
    [
      ["Import model…", "⌘I"],
      ["Open project…", "⌘O"],
      "-",
      ["Save project", "⌘S"],
      ["Export G-code…", "⌘E"],
      "-",
      ["Recent projects", ""],
    ],
  ],
  [
    "Edit",
    [
      ["Undo move", "⌘Z"],
      ["Redo", "⇧⌘Z"],
      "-",
      ["Duplicate", "⌘D"],
      ["Delete model", "⌫"],
      ["Select all", "⌘A"],
    ],
  ],
  [
    "View",
    [
      ["Prepare", "⌘1"],
      ["Preview", "⌘2"],
      "-",
      ["Show build volume", ""],
      ["Show travel moves", ""],
      ["Reset camera", "⇧R"],
    ],
  ],
  [
    "Help",
    [
      ["Documentation", "F1"],
      ["Keyboard shortcuts", "⌘K"],
      "-",
      ["About Meridian", ""],
    ],
  ],
]

export default function View() {
  return (
    <TooltipProvider>
      <div className="flex h-[800px] w-[1280px] flex-col overflow-hidden bg-background font-sans text-foreground antialiased">
        {/* ── title / menu bar ─────────────────────────────────── */}
        <header className="flex h-9 shrink-0 items-center gap-1 border-b bg-card pr-1.5 pl-2">
          <div className="flex items-center gap-2 pr-2">
            <div className="grid size-5 shrink-0 place-items-center rounded-[5px] bg-primary text-primary-foreground">
              <Layers className="size-3" />
            </div>
            <span className="text-[13px] leading-none font-semibold tracking-tight">
              Meridian{" "}
              <span className="font-normal text-muted-foreground">Slicer</span>
            </span>
          </div>

          <Separator orientation="vertical" className="h-4!" />

          <Menubar className="h-7 gap-0 border-0 bg-transparent p-0 shadow-none">
            {MENUS.map(([name, items]) => (
              <MenubarMenu key={name}>
                <MenubarTrigger className="px-2 py-1 text-xs font-normal text-muted-foreground data-[state=open]:text-foreground">
                  {name}
                </MenubarTrigger>
                <MenubarContent className="min-w-[13rem]">
                  {items.map((entry, i) =>
                    entry === "-" ? (
                      <MenubarSeparator key={i} />
                    ) : (
                      <MenubarItem key={entry[0]} className="text-xs">
                        {entry[0]}
                        {entry[1] && (
                          <MenubarShortcut>{entry[1]}</MenubarShortcut>
                        )}
                      </MenubarItem>
                    )
                  )}
                </MenubarContent>
              </MenubarMenu>
            ))}
          </Menubar>

          <div className="flex-1" />

          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                variant="ghost"
                size="sm"
                className="h-7 gap-1.5 px-2 text-xs font-normal text-muted-foreground hover:text-foreground"
              >
                <Printer className="size-3.5" />
                Ender-3 S1 Pro
                <ChevronDown className="size-3 opacity-60" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="min-w-[15rem]">
              <DropdownMenuLabel className="text-xs">
                Active printer
              </DropdownMenuLabel>
              <DropdownMenuRadioGroup value="ender3">
                <DropdownMenuRadioItem value="ender3" className="text-xs">
                  Ender-3 S1 Pro
                </DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="mk4" className="text-xs">
                  Prusa MK4S
                </DropdownMenuRadioItem>
                <DropdownMenuRadioItem value="x1c" className="text-xs">
                  Bambu Lab X1 Carbon
                </DropdownMenuRadioItem>
              </DropdownMenuRadioGroup>
              <DropdownMenuSeparator />
              <DropdownMenuItem className="text-xs">
                Manage printers…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          <Separator orientation="vertical" className="mx-1 h-4!" />

          <ButtonGroup>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button size="sm" className="h-7 gap-1.5 px-3 text-xs">
                  <Layers className="size-3.5" />
                  Slice
                </Button>
              </TooltipTrigger>
              <TooltipContent side="bottom" className="flex items-center gap-2">
                Slice model
                <KbdGroup>
                  <Kbd>⌘</Kbd>
                  <Kbd>P</Kbd>
                </KbdGroup>
              </TooltipContent>
            </Tooltip>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button size="sm" className="h-7 w-6 px-0">
                  <ChevronDown className="size-3" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                <DropdownMenuItem className="text-xs">
                  Slice and preview
                  <DropdownMenuShortcut>⌘P</DropdownMenuShortcut>
                </DropdownMenuItem>
                <DropdownMenuItem className="text-xs">
                  Slice and export G-code
                  <DropdownMenuShortcut>⇧⌘E</DropdownMenuShortcut>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem className="text-xs">
                  Send to printer…
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </ButtonGroup>
        </header>

        {/* ── body ─────────────────────────────────────────────── */}
        <div className="flex min-h-0 flex-1">
          {/* tool rail */}
          <nav className="flex w-11 shrink-0 flex-col items-center gap-1 border-r bg-card py-2">
            <ToggleGroup
              type="single"
              defaultValue="move"
              spacing={1}
              className="flex-col"
            >
              {TOOLS.map(([value, name, Icon, key]) => (
                <ToolTip
                  key={value}
                  side="right"
                  label={
                    <>
                      {name}
                      <Kbd>{key}</Kbd>
                    </>
                  }
                >
                  <ToggleGroupItem
                    value={value}
                    aria-label={name}
                    className="relative size-8 px-0 text-muted-foreground/80 before:absolute before:top-1.5 before:bottom-1.5 before:-left-[6px] before:w-[2px] before:rounded-full before:bg-primary before:opacity-0 data-[state=on]:text-foreground data-[state=on]:before:opacity-100"
                  >
                    <Icon className="size-4" />
                  </ToggleGroupItem>
                </ToolTip>
              ))}
            </ToggleGroup>

            <div className="mt-auto">
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="text-muted-foreground"
                  >
                    <Settings2 className="size-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="right">Preferences</TooltipContent>
              </Tooltip>
            </div>
          </nav>

          {/* viewport */}
          <main className="relative min-w-0 flex-1 overflow-hidden bg-background">
            {/* build-plate grid */}
            <div
              className="absolute inset-0"
              style={{
                backgroundImage:
                  "linear-gradient(to right, rgba(255,255,255,0.05) 1px, transparent 1px), linear-gradient(to bottom, rgba(255,255,255,0.05) 1px, transparent 1px)",
                backgroundSize: "44px 44px",
                backgroundPosition: "center center",
              }}
            />
            <div
              className="absolute inset-0"
              style={{
                background:
                  "radial-gradient(60% 55% at 50% 48%, rgba(255,255,255,0.035), transparent 70%)",
              }}
            />

            {/* centered placeholder */}
            <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 select-none">
              <Cuboid
                className="size-10 text-muted-foreground/25"
                strokeWidth={1.25}
              />
              <div className="text-center">
                <div className="font-mono text-sm text-muted-foreground/45">
                  {MODEL}
                </div>
                <div className="mt-1 font-mono text-[11px] text-muted-foreground/25">
                  128.4 × 96.0 × 42.5 mm
                </div>
              </div>
            </div>

            {/* corner overlay: camera presets + view toggles */}
            <div className="absolute top-3 left-3 flex items-center gap-1.5">
              <ToggleGroup
                type="single"
                defaultValue="iso"
                variant="outline"
                className="bg-card/85 shadow-xs backdrop-blur-sm"
              >
                {["Front", "Top", "Left", "Iso"].map((v) => (
                  <ToggleGroupItem
                    key={v}
                    value={v.toLowerCase()}
                    className="h-7 px-2.5 text-[11px] font-normal text-muted-foreground data-[state=on]:text-foreground"
                  >
                    {v}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>

              <ToggleGroup
                type="multiple"
                defaultValue={["grid"]}
                variant="outline"
                className="bg-card/85 shadow-xs backdrop-blur-sm"
              >
                {/* no tooltip wrapper here: it would break the segmented
                    first/last rounding of a spacing=0 toggle group */}
                <ToggleGroupItem
                  value="grid"
                  aria-label="Toggle build plate grid"
                  title="Build plate grid"
                  className="size-7 px-0 text-muted-foreground data-[state=on]:text-foreground"
                >
                  <Grid2x2 className="size-3.5" />
                </ToggleGroupItem>
                <ToggleGroupItem
                  value="supports"
                  aria-label="Toggle supports"
                  title="Show supports"
                  className="size-7 px-0 text-muted-foreground data-[state=on]:text-foreground"
                >
                  <Blocks className="size-3.5" />
                </ToggleGroupItem>
              </ToggleGroup>
            </div>

            {/* layer scrubber */}
            <div className="absolute top-4 right-3 bottom-9 flex w-7 flex-col items-center gap-2">
              <span className="font-mono text-[10px] text-muted-foreground/60 tabular-nums">
                412
              </span>
              <Slider
                orientation="vertical"
                defaultValue={[412]}
                min={1}
                max={412}
                className="min-h-0 flex-1 [&_[data-slot=slider-range]]:bg-muted-foreground/40 [&_[data-slot=slider-thumb]]:size-2.5 [&_[data-slot=slider-thumb]]:border-muted-foreground [&_[data-slot=slider-thumb]]:bg-foreground [&_[data-slot=slider-track]]:w-1 [&_[data-slot=slider-track]]:bg-secondary"
                aria-label="Layer"
              />
              <span className="font-mono text-[10px] text-muted-foreground/60 tabular-nums">
                1
              </span>
            </div>

            {/* corner readouts */}
            <div className="absolute bottom-3 left-3 font-mono text-[11px] text-muted-foreground/45 select-none">
              Ender-3 S1 Pro · build volume 220 × 220 × 270 mm
            </div>
            <div className="absolute bottom-3 right-11 font-mono text-[11px] text-muted-foreground/45 select-none">
              1 object · 24,918 triangles
            </div>
          </main>

          {/* settings panel */}
          <aside className="flex w-[300px] shrink-0 flex-col border-l bg-card">
            <div className="flex h-9 shrink-0 items-center gap-2 border-b pr-1.5 pl-3">
              <span className="flex-1 text-xs font-semibold tracking-tight">
                Print Settings
              </span>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    className="text-muted-foreground"
                  >
                    <RotateCcw />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom">Reset to profile</TooltipContent>
              </Tooltip>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    className="text-muted-foreground"
                  >
                    <Save />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom">Save profile</TooltipContent>
              </Tooltip>
            </div>

            {/* loaded model */}
            <Item
              size="sm"
              className="shrink-0 gap-2.5 rounded-none border-b border-border/70 px-3 py-2"
            >
              <ItemMedia
                variant="icon"
                className="size-7 border-border/60 bg-secondary/40 text-muted-foreground"
              >
                <Cuboid className="size-3.5" />
              </ItemMedia>
              <ItemContent className="gap-0.5">
                <ItemTitle className="text-xs font-medium">{MODEL}</ItemTitle>
                <ItemDescription className="font-mono text-[11px] leading-none">
                  41.3 cm³ · scale 100 %
                </ItemDescription>
              </ItemContent>
              <ItemActions>
                <Button
                  variant="ghost"
                  size="icon-xs"
                  className="text-muted-foreground"
                >
                  <Eye />
                </Button>
              </ItemActions>
            </Item>

            {/* profile */}
            <div className="shrink-0 border-b border-border/70 px-3 py-2">
              <div className="mb-1.5 flex items-center justify-between">
                <span className="text-[10px] font-semibold tracking-[0.09em] text-muted-foreground uppercase">
                  Profile
                </span>
                <Badge
                  variant="outline"
                  className="h-4 rounded-sm border-border/70 px-1 text-[9px] font-medium tracking-wide text-muted-foreground uppercase"
                >
                  Modified
                </Badge>
              </div>
              <Select defaultValue="standard">
                <SelectTrigger className="h-7! w-full px-2 text-xs">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="draft" className="text-xs">
                    Draft — 0.28 mm
                  </SelectItem>
                  <SelectItem value="standard" className="text-xs">
                    Standard — 0.20 mm
                  </SelectItem>
                  <SelectItem value="quality" className="text-xs">
                    Quality — 0.12 mm
                  </SelectItem>
                  <SelectItem value="ultra" className="text-xs">
                    Ultra fine — 0.08 mm
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <ScrollArea className="min-h-0 flex-1">
              <PaneSection title="Quality">
                <Row label="Layer height">
                  <MiniSelect
                    defaultValue="0.20"
                    label="Layer height"
                    options={[
                      ["0.08", "0.08 mm"],
                      ["0.12", "0.12 mm"],
                      ["0.16", "0.16 mm"],
                      ["0.20", "0.20 mm"],
                      ["0.28", "0.28 mm"],
                    ]}
                  />
                </Row>
                <Row label="Initial layer height">
                  <NumField value="0.24" unit="mm" />
                </Row>
                <Row label="Wall count">
                  <NumField value="3" />
                </Row>
                <Row label="Top / bottom layers">
                  <NumField value="5" />
                </Row>
              </PaneSection>

              <PaneSection title="Infill">
                <Row label="Density">
                  <NumField value="24" unit="%" />
                </Row>
                <div className="px-3 pt-0.5 pb-2">
                  <Slider
                    defaultValue={[24]}
                    max={100}
                    step={1}
                    aria-label="Infill density"
                    className="[&_[data-slot=slider-thumb]]:size-3 [&_[data-slot=slider-track]]:h-1"
                  />
                </div>
                <Row label="Pattern">
                  <MiniSelect
                    defaultValue="gyroid"
                    label="Infill pattern"
                    options={[
                      ["grid", "Grid"],
                      ["lines", "Lines"],
                      ["triangles", "Triangles"],
                      ["cubic", "Cubic"],
                      ["gyroid", "Gyroid"],
                      ["honeycomb", "Honeycomb"],
                    ]}
                  />
                </Row>
              </PaneSection>

              <PaneSection title="Material">
                <Row label="Material">
                  <MiniSelect
                    defaultValue="pla"
                    label="Filament"
                    options={[
                      ["pla", "PLA Basic"],
                      ["petg", "PETG"],
                      ["abs", "ABS"],
                      ["asa", "ASA"],
                      ["tpu", "TPU 95A"],
                    ]}
                  />
                </Row>
                <Row label="Nozzle temperature">
                  <NumField value="210" unit="°C" />
                </Row>
                <Row label="Bed temperature">
                  <NumField value="60" unit="°C" />
                </Row>
              </PaneSection>

              <PaneSection title="Supports">
                <Row label="Generate supports">
                  <Checkbox defaultChecked aria-label="Generate supports" />
                </Row>
                <Row label="Overhang angle">
                  <NumField value="55" unit="°" />
                </Row>
                <Row label="Structure">
                  <MiniSelect
                    defaultValue="tree"
                    label="Support structure"
                    options={[
                      ["normal", "Normal"],
                      ["tree", "Tree"],
                    ]}
                  />
                </Row>
                <Row label="Placement">
                  <MiniSelect
                    defaultValue="everywhere"
                    label="Placement"
                    options={[
                      ["everywhere", "Everywhere"],
                      ["plate", "Touching plate"],
                    ]}
                  />
                </Row>
              </PaneSection>
            </ScrollArea>
          </aside>
        </div>

        {/* ── status bar ───────────────────────────────────────── */}
        <footer className="flex h-7 shrink-0 items-center gap-2.5 border-t bg-card px-2 text-[11px] text-muted-foreground select-none">
          <span className="flex items-center gap-1.5 text-foreground">
            <span className="size-1.5 rounded-full bg-emerald-400" />
            Ready
          </span>
          <Separator orientation="vertical" className="h-3.5!" />
          <span className="flex items-center gap-1.5">
            <Cuboid className="size-3" />
            <span className="font-mono">{MODEL}</span>
          </span>
          <Separator orientation="vertical" className="h-3.5!" />
          <span className="font-mono">PLA · 0.4 mm nozzle</span>

          <div className="flex-1" />

          <Badge
            variant="secondary"
            className="h-5 gap-1 rounded-sm px-1.5 font-mono text-[11px] font-normal tabular-nums"
          >
            <Layers className="text-muted-foreground" />
            412 layers
          </Badge>
          <Badge
            variant="secondary"
            className="h-5 gap-1 rounded-sm px-1.5 font-mono text-[11px] font-normal tabular-nums"
          >
            <Clock className="text-muted-foreground" />
            4 h 12 min
          </Badge>
          <Badge
            variant="secondary"
            className="h-5 gap-1 rounded-sm px-1.5 font-mono text-[11px] font-normal tabular-nums"
          >
            <Spool className="text-muted-foreground" />
            18.42 m · 54.9 g
          </Badge>
        </footer>
      </div>
    </TooltipProvider>
  )
}
