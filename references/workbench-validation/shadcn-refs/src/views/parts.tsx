import { Fragment } from "react"
import {
  Check,
  CircuitBoard,
  CloudCheck,
  Columns3,
  Ellipsis,
  Link as LinkIcon,
  Plus,
  RotateCcw,
  Search,
  SlidersHorizontal,
  TriangleAlert,
} from "lucide-react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group"
import { Input } from "@/components/ui/input"
import { Kbd } from "@/components/ui/kbd"
import { ScrollArea } from "@/components/ui/scroll-area"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectSeparator,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { Separator } from "@/components/ui/separator"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

type Part = {
  ref: string
  value: string
  footprint: string
  library: string
  stock: number
}

type Group = { label: string; parts: Part[] }

const GROUPS: Group[] = [
  {
    label: "Resistors",
    parts: [
      { ref: "RC0603FR-0710KL", value: "10 kΩ 1%", footprint: "0603", library: "Yageo_RC", stock: 4812 },
      { ref: "RC0603FR-07100RL", value: "100 Ω 1%", footprint: "0603", library: "Yageo_RC", stock: 2140 },
      { ref: "RC0402FR-074K7L", value: "4.7 kΩ 1%", footprint: "0402", library: "Yageo_RC", stock: 8964 },
      { ref: "CRCW08051K00FKEA", value: "1 kΩ 1%", footprint: "0805", library: "Vishay_CRCW", stock: 1265 },
      { ref: "WSL2512R0100FEA", value: "10 mΩ 1%", footprint: "2512", library: "Vishay_Shunt", stock: 96 },
    ],
  },
  {
    label: "Capacitors",
    parts: [
      { ref: "GRM188R71H104KA93D", value: "100 nF 50 V", footprint: "0603", library: "Murata_GRM", stock: 12450 },
      { ref: "CL10A106MQ8NNNC", value: "10 µF 6.3 V", footprint: "0603", library: "Samsung_CL", stock: 3318 },
      { ref: "GRM155R61A105KE15D", value: "1 µF 10 V", footprint: "0402", library: "Murata_GRM", stock: 5960 },
      { ref: "C0805C225K3RACTU", value: "2.2 µF 25 V", footprint: "0805", library: "KEMET_C", stock: 1704 },
      { ref: "UWT1V101MCL1GS", value: "100 µF 35 V", footprint: "CP_Elec_6.3x5.4", library: "Nichicon_UWT", stock: 212 },
    ],
  },
  {
    label: "Semiconductors",
    parts: [
      { ref: "STM32G031K8T6", value: "MCU 64 kB M0+", footprint: "LQFP-32", library: "ST_MCU_G0", stock: 148 },
      { ref: "AP2112K-3.3TRG1", value: "LDO 3.3 V 600 mA", footprint: "SOT-23-5", library: "Diodes_Reg", stock: 1020 },
      { ref: "TLV9061IDBVR", value: "Op-amp 10 MHz", footprint: "SOT-23-5", library: "TI_Amplifier", stock: 830 },
      { ref: "BC847B", value: "NPN 45 V 100 mA", footprint: "SOT-23", library: "Nexperia_BJT", stock: 6407 },
      { ref: "1N4148WS", value: "Diode 75 V 150 mA", footprint: "SOD-323", library: "Diodes_Small", stock: 3880 },
      { ref: "SS34", value: "Schottky 40 V 3 A", footprint: "DO-214AC", library: "Vishay_Diode", stock: 174 },
    ],
  },
  {
    label: "Connectors & timing",
    parts: [
      { ref: "USB4110-GF-A", value: "USB-C recept. 2.0", footprint: "USB_C_SMD_16P", library: "GCT_USB", stock: 318 },
      { ref: "ABM8-24.000MHZ-B2-T", value: "24 MHz 10 ppm", footprint: "Crystal_3225-4", library: "Abracon_XTAL", stock: 476 },
    ],
  },
]

const SELECTED = "STM32G031K8T6"
const LOW_STOCK = 250

const COLS = [
  { key: "reference", label: "Reference", width: "w-auto" },
  { key: "value", label: "Value", width: "w-[152px]" },
  { key: "footprint", label: "Footprint", width: "w-[150px]" },
  { key: "library", label: "Library", width: "w-[148px]" },
  { key: "stock", label: "Stock", width: "w-[96px]" },
]

/** Pads per side of the LQFP-32 footprint sketch. */
const PADS = Array.from({ length: 8 }, (_, i) => i)

function FootprintPreview() {
  return (
    <div
      className="relative mx-auto size-[160px] overflow-hidden rounded-sm border border-border bg-[radial-gradient(circle,rgb(255_255_255/0.08)_1px,transparent_1px)] [background-position:4px_4px] [background-size:8px_8px]"
      role="img"
      aria-label="LQFP-32 footprint preview"
    >
      {/* origin crosshair */}
      <div className="absolute inset-x-0 top-1/2 h-px bg-white/8" />
      <div className="absolute inset-y-0 left-1/2 w-px bg-white/8" />

      {/* copper pads */}
      <div className="absolute inset-x-[44px] top-[20px] flex justify-between">
        {PADS.map((i) => (
          <div key={`t${i}`} className="h-[16px] w-[4px] rounded-[1px] bg-amber-600/70" />
        ))}
      </div>
      <div className="absolute inset-x-[44px] bottom-[20px] flex justify-between">
        {PADS.map((i) => (
          <div key={`b${i}`} className="h-[16px] w-[4px] rounded-[1px] bg-amber-600/70" />
        ))}
      </div>
      <div className="absolute inset-y-[44px] left-[20px] flex flex-col justify-between">
        {PADS.map((i) => (
          <div key={`l${i}`} className="h-[4px] w-[16px] rounded-[1px] bg-amber-600/70" />
        ))}
      </div>
      <div className="absolute inset-y-[44px] right-[20px] flex flex-col justify-between">
        {PADS.map((i) => (
          <div key={`r${i}`} className="h-[4px] w-[16px] rounded-[1px] bg-amber-600/70" />
        ))}
      </div>

      {/* silkscreen body + pin-1 marker */}
      <div className="absolute inset-[36px] rounded-[2px] border border-zinc-400/40" />
      <div className="absolute top-[42px] left-[42px] size-[6px] rounded-full border border-zinc-400/55" />
      <div className="absolute inset-0 flex items-center justify-center">
        <span className="font-mono text-[10px] tracking-wide text-muted-foreground">U3</span>
      </div>
    </div>
  )
}

function StockCell({ stock }: { stock: number }) {
  const low = stock < LOW_STOCK
  return (
    <span
      className={
        "flex items-center justify-end gap-1 font-mono text-xs tabular-nums " +
        (low ? "text-destructive" : "text-muted-foreground")
      }
    >
      {low && <TriangleAlert className="size-3" />}
      {stock.toLocaleString("en-US")}
    </span>
  )
}

export default function View() {
  return (
    <div className="flex h-full w-full flex-col overflow-hidden bg-background font-sans text-foreground">
      {/* ── toolbar ───────────────────────────────────────────────── */}
      <header className="flex h-12 shrink-0 items-center gap-2 border-b bg-card px-3">
        <div className="flex items-center gap-2 pr-1">
          <CircuitBoard className="size-4 text-amber-500" />
          <span className="text-sm font-semibold tracking-tight">Copperline</span>
        </div>
        <Separator orientation="vertical" className="mx-1 !h-5" />

        <InputGroup className="h-8 flex-1">
          <InputGroupAddon>
            <Search />
          </InputGroupAddon>
          <InputGroupInput
            placeholder="Search parts by MPN, value, footprint…"
            className="h-8 text-sm"
          />
          <InputGroupAddon align="inline-end">
            <Kbd>⌘K</Kbd>
          </InputGroupAddon>
        </InputGroup>

        <Select defaultValue="all">
          <SelectTrigger size="sm" className="w-[178px] text-xs">
            <SelectValue placeholder="Library" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All libraries</SelectItem>
            <SelectSeparator />
            <SelectItem value="yageo">Yageo_RC</SelectItem>
            <SelectItem value="murata">Murata_GRM</SelectItem>
            <SelectItem value="st">ST_MCU_G0</SelectItem>
            <SelectItem value="ti">TI_Amplifier</SelectItem>
          </SelectContent>
        </Select>

        <Select defaultValue="all">
          <SelectTrigger size="sm" className="w-[152px] text-xs">
            <SelectValue placeholder="Package" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="all">All packages</SelectItem>
            <SelectSeparator />
            <SelectItem value="0402">0402</SelectItem>
            <SelectItem value="0603">0603</SelectItem>
            <SelectItem value="0805">0805</SelectItem>
            <SelectItem value="sot23">SOT-23</SelectItem>
            <SelectItem value="lqfp32">LQFP-32</SelectItem>
          </SelectContent>
        </Select>

        <Button variant="ghost" size="icon-sm" aria-label="Advanced filters">
          <SlidersHorizontal />
        </Button>
        <Separator orientation="vertical" className="mx-1 !h-5" />
        <Button size="sm">
          <Plus />
          Add Part
        </Button>
      </header>

      {/* ── body split ────────────────────────────────────────────── */}
      <div className="flex min-h-0 flex-1">
        {/* parts table */}
        <section className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-[30px] shrink-0 items-center gap-2 border-b bg-card/40 px-3">
            <span className="text-[11px] font-medium tracking-[0.08em] text-muted-foreground uppercase">
              Parts
            </span>
            <Badge
              variant="outline"
              className="h-4 rounded-sm border-border/60 px-1 font-mono text-[10px] font-normal text-muted-foreground"
            >
              412
            </Badge>
            <div className="flex-1" />
            <span className="font-mono text-[10px] text-muted-foreground">
              sort: reference ↑
            </span>
            <Button variant="ghost" size="icon-xs" aria-label="Configure columns">
              <Columns3 />
            </Button>
          </div>

          <ScrollArea className="min-h-0 flex-1">
            <Table className="table-fixed border-separate border-spacing-0">
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead className="h-8 w-[38px] border-b bg-background px-3">
                    <Checkbox aria-label="Select all" />
                  </TableHead>
                  {COLS.map((c) => (
                    <TableHead
                      key={c.key}
                      className={`h-8 border-b bg-background px-3 text-[11px] font-medium tracking-[0.06em] text-muted-foreground uppercase ${c.width} ${
                        c.key === "stock" ? "text-right" : ""
                      }`}
                    >
                      {c.label}
                    </TableHead>
                  ))}
                </TableRow>
              </TableHeader>
              <TableBody>
                {GROUPS.map((group) => (
                  <Fragment key={group.label}>
                    <TableRow className="hover:bg-muted/30">
                      <TableCell
                        colSpan={6}
                        className="h-[25px] border-b bg-muted/30 px-3 py-0 text-[10px] font-medium tracking-[0.1em] text-muted-foreground uppercase"
                      >
                        {group.label}
                        <span className="ml-2 font-mono text-[10px] tracking-normal normal-case opacity-70">
                          {group.parts.length}
                        </span>
                      </TableCell>
                    </TableRow>
                    {group.parts.map((p) => {
                      const selected = p.ref === SELECTED
                      return (
                        <TableRow
                          key={p.ref}
                          data-state={selected ? "selected" : undefined}
                          className="[&>td]:border-b"
                        >
                          <TableCell
                            className={`h-[30px] px-3 py-0 ${
                              selected
                                ? "border-l-2 border-l-amber-500"
                                : "border-l-2 border-l-transparent"
                            }`}
                          >
                            <Checkbox
                              defaultChecked={selected}
                              aria-label={`Select ${p.ref}`}
                            />
                          </TableCell>
                          <TableCell
                            className={`h-[30px] truncate px-3 py-0 font-mono text-xs ${
                              selected ? "font-medium text-foreground" : "text-foreground/90"
                            }`}
                          >
                            {p.ref}
                          </TableCell>
                          <TableCell className="h-[30px] truncate px-3 py-0 text-xs">
                            {p.value}
                          </TableCell>
                          <TableCell className="h-[30px] truncate px-3 py-0 font-mono text-xs text-muted-foreground">
                            {p.footprint}
                          </TableCell>
                          <TableCell className="h-[30px] truncate px-3 py-0 font-mono text-xs text-muted-foreground">
                            {p.library}
                          </TableCell>
                          <TableCell className="h-[30px] px-3 py-0 text-right">
                            <StockCell stock={p.stock} />
                          </TableCell>
                        </TableRow>
                      )
                    })}
                  </Fragment>
                ))}
              </TableBody>
            </Table>
          </ScrollArea>
        </section>

        {/* inspector */}
        <aside className="flex w-[320px] shrink-0 flex-col border-l bg-card/40">
          <div className="flex h-[30px] shrink-0 items-center gap-2 border-b px-3">
            <span className="text-[11px] font-medium tracking-[0.08em] text-muted-foreground uppercase">
              Properties
            </span>
            <div className="flex-1" />
            <Button variant="ghost" size="icon-xs" aria-label="Pane actions">
              <Ellipsis />
            </Button>
          </div>

          <ScrollArea className="min-h-0 flex-1">
            {/* footprint preview */}
            <div className="border-b px-3 py-2">
              <div className="mb-2 flex items-center justify-between">
                <ToggleGroup
                  type="single"
                  defaultValue="top"
                  variant="outline"
                  size="sm"
                  className="h-7"
                >
                  <ToggleGroupItem value="top" className="h-7 px-2 text-[11px]">
                    Top
                  </ToggleGroupItem>
                  <ToggleGroupItem value="bottom" className="h-7 px-2 text-[11px]">
                    Bottom
                  </ToggleGroupItem>
                  <ToggleGroupItem value="3d" className="h-7 px-2 text-[11px]">
                    3D
                  </ToggleGroupItem>
                </ToggleGroup>
                <span className="font-mono text-[10px] text-muted-foreground">1:1</span>
              </div>
              <FootprintPreview />
              <p className="mt-1.5 text-center font-mono text-[10px] text-muted-foreground">
                LQFP-32 · 0.8 mm pitch · 7.0 × 7.0 mm
              </p>
            </div>

            {/* read-only identity */}
            <dl className="grid grid-cols-[68px_1fr] gap-x-2 gap-y-1 border-b px-3 py-2 text-[11px]">
              <dt className="tracking-wide text-muted-foreground uppercase">MPN</dt>
              <dd className="truncate font-mono">STM32G031K8T6</dd>
              <dt className="tracking-wide text-muted-foreground uppercase">Mfr</dt>
              <dd className="truncate">STMicroelectronics</dd>
              <dt className="tracking-wide text-muted-foreground uppercase">Symbol</dt>
              <dd className="truncate font-mono">MCU_ST_STM32G0:STM32G031K8Tx</dd>
              <dt className="tracking-wide text-muted-foreground uppercase">Updated</dt>
              <dd className="truncate font-mono">2026-06-14 · rev 4</dd>
            </dl>

            {/* editable fields */}
            <FieldGroup className="gap-2.5 px-3 py-2.5">
              <Field>
                <FieldLabel
                  htmlFor="part-value"
                  className="text-[11px] tracking-wide text-muted-foreground uppercase"
                >
                  Value
                </FieldLabel>
                <Input id="part-value" defaultValue="MCU 64 kB M0+" className="h-8 text-xs" />
              </Field>

              <Field>
                <FieldLabel
                  htmlFor="part-footprint"
                  className="text-[11px] tracking-wide text-muted-foreground uppercase"
                >
                  Footprint
                </FieldLabel>
                <Select defaultValue="lqfp32">
                  <SelectTrigger
                    id="part-footprint"
                    size="sm"
                    className="w-full font-mono text-xs"
                  >
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="lqfp32">LQFP-32_7x7mm_P0.8mm</SelectItem>
                    <SelectItem value="ufqfpn32">UFQFPN-32_5x5mm_P0.5mm</SelectItem>
                    <SelectItem value="tssop20">TSSOP-20_4.4x6.5mm_P0.65mm</SelectItem>
                  </SelectContent>
                </Select>
              </Field>

              <Field>
                <FieldLabel
                  htmlFor="part-datasheet"
                  className="text-[11px] tracking-wide text-muted-foreground uppercase"
                >
                  Datasheet URL
                </FieldLabel>
                <InputGroup className="h-8">
                  <InputGroupAddon>
                    <LinkIcon />
                  </InputGroupAddon>
                  <InputGroupInput
                    id="part-datasheet"
                    defaultValue="st.com/…/stm32g031k8.pdf"
                    className="h-8 font-mono text-[11px]"
                  />
                  <InputGroupAddon align="inline-end">
                    <InputGroupButton size="icon-xs" aria-label="Open datasheet">
                      <LinkIcon />
                    </InputGroupButton>
                  </InputGroupAddon>
                </InputGroup>
              </Field>

              <Field>
                <FieldLabel
                  htmlFor="part-description"
                  className="text-[11px] tracking-wide text-muted-foreground uppercase"
                >
                  Description
                </FieldLabel>
                <Textarea
                  id="part-description"
                  className="min-h-[58px] resize-none text-xs leading-snug"
                  defaultValue="Cortex-M0+ 64 MHz, 64 kB flash, 8 kB SRAM. 100 nF per VDD pin."
                />
              </Field>
            </FieldGroup>
          </ScrollArea>

          <div className="flex h-11 shrink-0 items-center justify-between gap-2 border-t px-3">
            <span className="text-[11px] text-muted-foreground">2 pending edits</span>
            <div className="flex items-center gap-2">
              <Button variant="outline" size="sm">
                <RotateCcw />
                Revert
              </Button>
              <Button size="sm">
                <Check />
                Apply
              </Button>
            </div>
          </div>
        </aside>
      </div>

      {/* ── status bar ────────────────────────────────────────────── */}
      <footer className="flex h-[26px] shrink-0 items-center gap-3 border-t bg-card px-3 text-[11px] text-muted-foreground">
        <span className="font-mono">412 parts · 1 selected</span>
        <div className="flex-1" />
        <Badge
          variant="outline"
          className="h-[18px] gap-1.5 rounded-sm border-border/60 px-1.5 font-normal text-muted-foreground"
        >
          <CloudCheck className="text-emerald-500" />
          Library synced · 2 min ago
        </Badge>
      </footer>
    </div>
  )
}
