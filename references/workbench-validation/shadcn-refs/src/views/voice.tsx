import {
  AtSign,
  AudioLines,
  Bell,
  ChevronDown,
  ChevronRight,
  Cpu,
  Headphones,
  Lock,
  Mic,
  MicOff,
  MonitorUp,
  Paperclip,
  Pin,
  Plus,
  Search,
  Send,
  Settings,
  Signal,
  Smile,
  UserPlus,
  Users,
  Volume2,
  VolumeX,
} from "lucide-react"

import { cn } from "@/lib/utils"
import {
  Avatar,
  AvatarBadge,
  AvatarFallback,
  AvatarGroup,
  AvatarGroupCount,
} from "@/components/ui/avatar"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuShortcut,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import {
  Menubar,
  MenubarCheckboxItem,
  MenubarContent,
  MenubarItem,
  MenubarMenu,
  MenubarSeparator,
  MenubarShortcut,
  MenubarTrigger,
} from "@/components/ui/menubar"
import { Progress } from "@/components/ui/progress"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Separator } from "@/components/ui/separator"
import { Toggle } from "@/components/ui/toggle"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"

/* ------------------------------------------------------------------ data */

type Presence = "idle" | "talking" | "muted" | "deafened"

type Member = {
  name: string
  presence: Presence
  self?: boolean
  streaming?: boolean
}

type Channel = {
  name: string
  members: Member[]
  collapsed?: boolean
  active?: boolean
  locked?: boolean
}

const CHANNELS: Channel[] = [
  {
    name: "Lobby",
    members: [
      { name: "mara.vasquez", presence: "idle" },
      { name: "tobin.reyes", presence: "muted" },
    ],
  },
  {
    name: "War Room",
    active: true,
    members: [
      { name: "kepner", presence: "idle", self: true },
      { name: "hazel.kim", presence: "talking" },
      { name: "dtorres", presence: "idle", streaming: true },
      { name: "priya.nandi", presence: "talking" },
      { name: "s.okafor", presence: "muted" },
      { name: "glenn.ferreira", presence: "idle" },
      { name: "nyx", presence: "deafened" },
    ],
  },
  {
    name: "Engineering Standup",
    members: [
      { name: "brandt", presence: "idle" },
      { name: "q.liang", presence: "muted" },
    ],
  },
  {
    name: "Pairing / Room 1",
    members: [{ name: "oz", presence: "talking" }],
  },
  {
    name: "Support Desk",
    collapsed: true,
    locked: true,
    members: [{ name: "amara.bello", presence: "idle" }],
  },
  {
    name: "Games — Deep Rock",
    members: [
      { name: "tobias.k", presence: "idle" },
      { name: "wren", presence: "muted" },
    ],
  },
  {
    name: "AFK",
    members: [
      { name: "vinh.tran", presence: "deafened" },
      { name: "rk.stone", presence: "deafened" },
    ],
  },
  {
    name: "Archive & Recordings",
    collapsed: true,
    locked: true,
    members: [],
  },
]

/** Author name colors — the one place hue is spent, so speakers stay
 *  scannable in a log that is otherwise monochrome. */
const AUTHOR_COLOR: Record<string, string> = {
  "hazel.kim": "text-violet-300",
  dtorres: "text-sky-300",
  "priya.nandi": "text-emerald-300",
  "glenn.ferreira": "text-amber-300",
  "s.okafor": "text-rose-300",
  nyx: "text-teal-300",
  kepner: "text-fuchsia-300",
}

const AVATAR_TINT: Record<string, string> = {
  "hazel.kim": "bg-violet-500/15 text-violet-300",
  dtorres: "bg-sky-500/15 text-sky-300",
  "priya.nandi": "bg-emerald-500/15 text-emerald-300",
  "s.okafor": "bg-rose-500/15 text-rose-300",
  kepner: "bg-fuchsia-500/15 text-fuchsia-300",
}

type Entry =
  | { kind: "divider"; label: string }
  | { kind: "system"; at: string; text: string }
  | { kind: "msg"; at: string; author?: string; text: string; mention?: string }

const LOG: Entry[] = [
  { kind: "divider", label: "Today · 26 July" },
  {
    kind: "msg",
    at: "09:26",
    author: "dtorres",
    text: "morning — I'm pushing the ingest patch to staging in a bit, will shout before the bridge restarts",
  },
  {
    kind: "msg",
    at: "09:27",
    author: "hazel.kim",
    text: "ack. I've still got 14:00 blocked for the codec swap, so anything touching the mixer should land before then",
  },
  {
    kind: "msg",
    at: "09:27",
    text: "opus at 64 kbit sounded rough on the packet-loss sim — I want to try 96 with the same jitter buffer",
  },
  {
    kind: "msg",
    at: "09:28",
    author: "s.okafor",
    text: "morning. I re-ran the fuzzer against the packet parser overnight, no new crashes",
  },
  {
    kind: "system",
    at: "09:29",
    text: "priya.nandi joined the channel — 41 ms, Opus 48 kHz",
  },
  {
    kind: "msg",
    at: "09:31",
    author: "priya.nandi",
    text: "loss-sim numbers are in the runbook. at 3% loss with the buffer at 40 ms it held; at 8% it starts eating consonants",
  },
  {
    kind: "msg",
    at: "09:32",
    text: "raising the buffer to 60 ms hides most of it, but you feel the lag in a back-and-forth",
  },
  {
    kind: "msg",
    at: "09:33",
    author: "kepner",
    mention: "@priya.nandi",
    text: "can you drop the link here? the one from friday 404s for me",
  },
  {
    kind: "msg",
    at: "09:33",
    author: "priya.nandi",
    text: "runbooks/voice/loss-sim-2026-07.md — I moved it out of scratch/ so the old link is dead",
  },
  {
    kind: "msg",
    at: "09:36",
    author: "glenn.ferreira",
    text: "unrelated: my push-to-talk rebinds to the wrong input every time the laptop wakes. anyone else seeing that?",
  },
  {
    kind: "msg",
    at: "09:37",
    author: "s.okafor",
    text: "yes — every time the dock re-enumerates. it's #412, we key devices by index instead of by id",
  },
  {
    kind: "msg",
    at: "09:38",
    author: "hazel.kim",
    text: "I'll pick up 412 right after the codec work. same subsystem, may as well do both in one pass",
  },
  {
    kind: "msg",
    at: "09:39",
    author: "nyx",
    text: "if you're in device enumeration anyway, please keep the pulse fallback — no pipewire on my box",
  },
  {
    kind: "msg",
    at: "09:41",
    author: "dtorres",
    text: "screen is up if anyone wants to watch the staging graphs with me",
  },
  { kind: "system", at: "09:44", text: "nyx set themselves to deafened" },
  {
    kind: "msg",
    at: "09:46",
    author: "glenn.ferreira",
    text: "thanks both — I'll add the dock model to the issue, it might matter",
  },
  {
    kind: "msg",
    at: "09:51",
    author: "dtorres",
    text: "deploying to staging now. voice bridge restarts, expect a two second blip",
  },
  {
    kind: "msg",
    at: "09:52",
    author: "kepner",
    text: "watching the jitter graph, say when",
  },
  {
    kind: "msg",
    at: "09:53",
    author: "hazel.kim",
    text: "bridge came back clean on my side, 48 ms and no reconnect",
  },
  {
    kind: "msg",
    at: "09:55",
    author: "priya.nandi",
    text: "good. I'll kick off the 96 kbit run at 14:00 and paste the graphs in here",
  },
  {
    kind: "msg",
    at: "09:56",
    author: "dtorres",
    text: "staging is green — ingest patch is in and nothing reconnected. thanks all",
  },
]

/* -------------------------------------------------------------- fragments */

function PresenceDot({ presence }: { presence: Presence }) {
  const tone =
    presence === "talking"
      ? "bg-emerald-400 ring-2 ring-emerald-400/25"
      : presence === "deafened"
        ? "bg-zinc-600"
        : presence === "muted"
          ? "bg-zinc-500"
          : "bg-zinc-400"
  return <span className={cn("size-1.5 shrink-0 rounded-full", tone)} />
}

function MemberRow({ member }: { member: Member }) {
  return (
    <div
      className={cn(
        "flex h-6 items-center gap-2 rounded-sm pr-1.5 pl-8 text-xs hover:bg-accent/40",
        member.self && "bg-accent/30"
      )}
    >
      <PresenceDot presence={member.presence} />
      <span
        className={cn(
          "truncate",
          member.presence === "talking"
            ? "text-foreground"
            : member.presence === "deafened"
              ? "text-muted-foreground/50"
              : "text-muted-foreground",
          member.self && "font-medium text-foreground"
        )}
      >
        {member.name}
        {member.self && (
          <span className="ml-1.5 text-[10px] text-muted-foreground/70">you</span>
        )}
      </span>
      <span className="ml-auto flex items-center gap-1 text-muted-foreground/60">
        {member.streaming && <MonitorUp className="size-3 text-sky-400/80" />}
        {member.presence === "talking" && (
          <AudioLines className="size-3 text-emerald-400" />
        )}
        {member.presence === "muted" && <MicOff className="size-3" />}
        {member.presence === "deafened" && (
          <>
            <MicOff className="size-3" />
            <VolumeX className="size-3" />
          </>
        )}
      </span>
    </div>
  )
}

function ChannelRow({ channel }: { channel: Channel }) {
  const Chevron = channel.collapsed ? ChevronRight : ChevronDown
  return (
    <div
      className={cn(
        "relative flex h-6 items-center gap-1.5 rounded-sm px-1.5 text-[13px] hover:bg-accent/40",
        channel.active && "bg-accent text-accent-foreground"
      )}
    >
      {channel.active && (
        <span className="absolute top-0.5 bottom-0.5 -left-1.5 w-[2px] rounded-full bg-sky-400" />
      )}
      <Chevron className="size-3 shrink-0 text-muted-foreground/70" />
      <Volume2
        className={cn(
          "size-3.5 shrink-0",
          channel.active ? "text-sky-400" : "text-muted-foreground/70"
        )}
      />
      <span
        className={cn(
          "truncate",
          channel.active ? "font-medium text-foreground" : "text-foreground/80"
        )}
      >
        {channel.name}
      </span>
      {channel.locked && (
        <Lock className="size-2.5 shrink-0 text-muted-foreground/50" />
      )}
      <span className="ml-auto font-mono text-[10px] text-muted-foreground/60 tabular-nums">
        {channel.members.length}
      </span>
    </div>
  )
}

function IconAction({
  label,
  children,
}: {
  label: string
  children: React.ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side="top" sideOffset={6}>
        {label}
      </TooltipContent>
    </Tooltip>
  )
}

function Meter({
  label,
  value,
  tone,
}: {
  label: string
  value: number
  tone: string
}) {
  return (
    <div className="flex items-center gap-1.5">
      <span className="font-mono text-[10px] text-muted-foreground/70">
        {label}
      </span>
      <Progress
        value={value}
        className={cn("h-1.5 w-14 rounded-sm bg-primary/10", tone)}
      />
    </div>
  )
}

function StatusChip({ children }: { children: React.ReactNode }) {
  return (
    <Badge
      variant="outline"
      className="h-5 gap-1 rounded-sm border-border/60 px-1.5 font-mono text-[10px] font-normal text-muted-foreground"
    >
      {children}
    </Badge>
  )
}

/* -------------------------------------------------------------------- view */

export default function View() {
  return (
    <TooltipProvider>
      <div className="flex h-[800px] w-[1280px] flex-col overflow-hidden bg-background font-sans text-foreground">
        {/* ---------------------------------------------- title / menu bar */}
        <div className="flex h-[30px] shrink-0 items-center gap-2 border-b bg-card px-2">
          <div className="flex items-center gap-1.5 pr-1">
            <AudioLines className="size-3.5 text-sky-400" />
            <span className="text-xs font-semibold tracking-tight">Banter</span>
          </div>
          <Separator orientation="vertical" className="h-4" />
          <Menubar className="h-auto gap-0 rounded-none border-0 bg-transparent p-0 shadow-none">
            <MenubarMenu>
              <MenubarTrigger className="px-2 py-0.5 text-xs font-normal">
                Server
              </MenubarTrigger>
              <MenubarContent>
                <MenubarItem>
                  Connect… <MenubarShortcut>⌘K</MenubarShortcut>
                </MenubarItem>
                <MenubarItem>Reconnect</MenubarItem>
                <MenubarSeparator />
                <MenubarItem>Server information</MenubarItem>
                <MenubarItem variant="destructive">Disconnect</MenubarItem>
              </MenubarContent>
            </MenubarMenu>
            <MenubarMenu>
              <MenubarTrigger className="px-2 py-0.5 text-xs font-normal">
                Self
              </MenubarTrigger>
              <MenubarContent>
                <MenubarCheckboxItem checked>
                  Transmit on push-to-talk
                </MenubarCheckboxItem>
                <MenubarCheckboxItem>Deafen output</MenubarCheckboxItem>
                <MenubarSeparator />
                <MenubarItem>
                  Set comment… <MenubarShortcut>⌘/</MenubarShortcut>
                </MenubarItem>
              </MenubarContent>
            </MenubarMenu>
            <MenubarMenu>
              <MenubarTrigger className="px-2 py-0.5 text-xs font-normal">
                Configure
              </MenubarTrigger>
              <MenubarContent>
                <MenubarItem>Audio wizard…</MenubarItem>
                <MenubarItem>Shortcuts…</MenubarItem>
                <MenubarSeparator />
                <MenubarItem>
                  Settings <MenubarShortcut>⌘,</MenubarShortcut>
                </MenubarItem>
              </MenubarContent>
            </MenubarMenu>
            <MenubarMenu>
              <MenubarTrigger className="px-2 py-0.5 text-xs font-normal">
                Help
              </MenubarTrigger>
              <MenubarContent>
                <MenubarItem>Documentation</MenubarItem>
                <MenubarItem>About Banter</MenubarItem>
              </MenubarContent>
            </MenubarMenu>
          </Menubar>
          <div className="ml-auto flex items-center gap-1.5">
            <InputGroup className="h-6 w-56 rounded-sm border-border/70 bg-input/20 shadow-none">
              <InputGroupAddon className="py-0 pl-2">
                <Search className="size-3 text-muted-foreground/70" />
              </InputGroupAddon>
              <InputGroupInput
                readOnly
                placeholder="Search messages"
                className="h-6 text-xs placeholder:text-muted-foreground/60 md:text-xs"
              />
              <InputGroupAddon align="inline-end" className="py-0 pr-1.5">
                <Kbd className="h-4 px-1 text-[10px]">⌘F</Kbd>
              </InputGroupAddon>
            </InputGroup>
            <IconAction label="Notifications">
              <Button
                variant="ghost"
                size="icon-xs"
                className="text-muted-foreground"
              >
                <Bell />
              </Button>
            </IconAction>
          </div>
        </div>

        {/* ------------------------------------------------ sidebar + main */}
        <div className="flex min-h-0 flex-1">
          {/* ---------------------------------------------------- sidebar */}
          <aside className="flex w-60 shrink-0 flex-col border-r bg-sidebar">
            <div className="flex h-11 shrink-0 items-center gap-2 border-b px-3">
              <div className="flex min-w-0 flex-col">
                <span className="truncate text-[13px] leading-4 font-semibold tracking-tight">
                  Northgate Ops
                </span>
                <span className="truncate font-mono text-[10px] text-muted-foreground">
                  voice.example.com:64738
                </span>
              </div>
              <DropdownMenu>
                <DropdownMenuTrigger asChild>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    className="ml-auto text-muted-foreground"
                  >
                    <ChevronDown />
                  </Button>
                </DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-52">
                  <DropdownMenuLabel>Northgate Ops</DropdownMenuLabel>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem>
                    <UserPlus /> Invite to server
                    <DropdownMenuShortcut>⌘I</DropdownMenuShortcut>
                  </DropdownMenuItem>
                  <DropdownMenuItem>
                    <Plus /> New channel
                  </DropdownMenuItem>
                  <DropdownMenuSeparator />
                  <DropdownMenuItem variant="destructive">
                    Disconnect
                  </DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>
            </div>

            <div className="flex h-7 shrink-0 items-center gap-1 px-3 text-[10px] font-medium tracking-wider text-muted-foreground/70 uppercase">
              Channels
              <Badge
                variant="secondary"
                className="ml-auto h-4 rounded-sm px-1 font-mono text-[10px] font-normal"
              >
                16 online
              </Badge>
            </div>

            <ScrollArea className="min-h-0 flex-1">
              <div className="space-y-px px-3 pb-2">
                {CHANNELS.map((channel) => (
                  <div key={channel.name} className="space-y-px">
                    <ChannelRow channel={channel} />
                    {!channel.collapsed &&
                      channel.members.map((member) => (
                        <MemberRow key={member.name} member={member} />
                      ))}
                  </div>
                ))}
              </div>
            </ScrollArea>

            {/* ------------------------------------------ user footer strip */}
            <div className="flex h-14 shrink-0 items-center gap-2 border-t bg-card px-3">
              <Avatar size="sm">
                <AvatarFallback className={cn("text-[10px]", AVATAR_TINT.kepner)}>
                  BK
                </AvatarFallback>
                <AvatarBadge className="bg-emerald-500" />
              </Avatar>
              <div className="flex min-w-0 flex-col">
                <span className="truncate text-xs font-medium">kepner</span>
                <span className="truncate text-[10px] text-muted-foreground">
                  Push-to-talk
                </span>
              </div>
              <div className="ml-auto flex items-center gap-0.5">
                <IconAction label="Microphone — transmitting">
                  <Toggle
                    defaultPressed
                    size="sm"
                    aria-label="Microphone"
                    className="size-7 min-w-7 rounded-sm p-0 data-[state=on]:bg-emerald-500/15 data-[state=on]:text-emerald-300"
                  >
                    <Mic className="size-3.5" />
                  </Toggle>
                </IconAction>
                <IconAction label="Deafen output">
                  <Toggle
                    size="sm"
                    aria-label="Headphones"
                    className="size-7 min-w-7 rounded-sm p-0 text-muted-foreground"
                  >
                    <Headphones className="size-3.5" />
                  </Toggle>
                </IconAction>
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      className="size-7 rounded-sm text-muted-foreground"
                    >
                      <Settings className="size-3.5" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end" side="top" className="w-56">
                    <DropdownMenuLabel>Audio devices</DropdownMenuLabel>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem>
                      <Mic /> Input
                      <DropdownMenuShortcut>Yeti X</DropdownMenuShortcut>
                    </DropdownMenuItem>
                    <DropdownMenuItem>
                      <Headphones /> Output
                      <DropdownMenuShortcut>DT 770</DropdownMenuShortcut>
                    </DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem>
                      <Settings /> Settings…
                      <DropdownMenuShortcut>⌘,</DropdownMenuShortcut>
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </div>
          </aside>

          {/* ------------------------------------------------------- main */}
          <main className="flex min-w-0 flex-1 flex-col">
            {/* --------------------------------------- channel header bar */}
            <div className="flex h-11 shrink-0 items-center gap-3 border-b bg-card px-4">
              <Volume2 className="size-4 shrink-0 text-sky-400" />
              <span className="shrink-0 text-sm font-semibold tracking-tight">
                War Room
              </span>
              <Separator orientation="vertical" className="h-4" />
              <span className="truncate text-xs text-muted-foreground">
                Deploy window 22:00 UTC — keep pushes off main until the bridge is
                green
              </span>
              <div className="ml-auto flex shrink-0 items-center gap-3">
                <AvatarGroup>
                  {["hazel.kim", "dtorres", "priya.nandi", "s.okafor"].map(
                    (name) => (
                      <Avatar key={name} size="sm">
                        <AvatarFallback
                          className={cn("text-[10px]", AVATAR_TINT[name])}
                        >
                          {name.slice(0, 2).toUpperCase()}
                        </AvatarFallback>
                      </Avatar>
                    )
                  )}
                  <AvatarGroupCount className="text-[10px]">+3</AvatarGroupCount>
                </AvatarGroup>
                <Badge
                  variant="secondary"
                  className="h-6 gap-1 rounded-sm px-1.5 font-mono text-[11px] font-normal"
                >
                  <Users className="size-3" />
                  7 / 25
                </Badge>
                <Separator orientation="vertical" className="h-4" />
                <IconAction label="Pinned messages">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="text-muted-foreground"
                  >
                    <Pin className="size-3.5" />
                  </Button>
                </IconAction>
                <IconAction label="Channel members">
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    className="text-muted-foreground"
                  >
                    <Users className="size-3.5" />
                  </Button>
                </IconAction>
              </div>
            </div>

            {/* ------------------------------------------------ message log */}
            <ScrollArea className="min-h-0 flex-1">
              <div className="flex flex-col py-2">
                {LOG.map((entry, i) => {
                  if (entry.kind === "divider") {
                    return (
                      <div
                        key={i}
                        className="flex items-center gap-3 px-4 py-3 select-none"
                      >
                        <Separator className="flex-1" />
                        <span className="font-mono text-[10px] tracking-wider text-muted-foreground/70 uppercase">
                          {entry.label}
                        </span>
                        <Separator className="flex-1" />
                      </div>
                    )
                  }
                  if (entry.kind === "system") {
                    return (
                      <div
                        key={i}
                        className="grid grid-cols-[48px_1fr] gap-x-3 px-4 py-[3px]"
                      >
                        <span className="font-mono text-[11px] leading-5 text-muted-foreground/45 tabular-nums">
                          {entry.at}
                        </span>
                        <p className="flex items-center gap-1.5 text-[12px] leading-5 text-muted-foreground/55 italic">
                          <Signal className="size-3 shrink-0 not-italic" />
                          {entry.text}
                        </p>
                      </div>
                    )
                  }
                  return (
                    <div
                      key={i}
                      className="grid grid-cols-[48px_1fr] gap-x-3 px-4 py-[3px] hover:bg-accent/25"
                    >
                      <span className="font-mono text-[11px] leading-5 text-muted-foreground/45 tabular-nums">
                        {entry.at}
                      </span>
                      <p className="text-[13px] leading-5 text-foreground/85">
                        {entry.author && (
                          <span
                            className={cn(
                              "mr-1.5 font-medium",
                              AUTHOR_COLOR[entry.author]
                            )}
                          >
                            {entry.author}
                          </span>
                        )}
                        {entry.mention && (
                          <span className="mr-1 rounded-sm bg-sky-400/10 px-1 font-medium text-sky-300">
                            {entry.mention}
                          </span>
                        )}
                        {entry.text}
                      </p>
                    </div>
                  )
                })}
              </div>
            </ScrollArea>

            {/* --------------------------------------------------- composer */}
            <div className="shrink-0 border-t bg-card px-4 pt-3 pb-2">
              <InputGroup className="h-10 rounded-md bg-input/20">
                <InputGroupAddon className="pl-2">
                  <InputGroupButton
                    size="icon-xs"
                    className="text-muted-foreground"
                    aria-label="Attach file"
                  >
                    <Paperclip />
                  </InputGroupButton>
                </InputGroupAddon>
                <InputGroupInput
                  readOnly
                  placeholder="Message War Room"
                  className="h-10 text-[13px] md:text-[13px]"
                />
                <InputGroupAddon align="inline-end" className="gap-1 pr-2">
                  <InputGroupButton
                    size="icon-xs"
                    className="text-muted-foreground"
                    aria-label="Mention someone"
                  >
                    <AtSign />
                  </InputGroupButton>
                  <InputGroupButton
                    size="icon-xs"
                    className="text-muted-foreground"
                    aria-label="Emoji"
                  >
                    <Smile />
                  </InputGroupButton>
                  <InputGroupButton
                    size="icon-sm"
                    variant="default"
                    className="bg-sky-600 text-white hover:bg-sky-500"
                    aria-label="Send message"
                  >
                    <Send className="size-3.5" />
                  </InputGroupButton>
                </InputGroupAddon>
              </InputGroup>
              <div className="flex h-5 items-center gap-2 pt-1 text-[10px] text-muted-foreground/70">
                <KbdGroup>
                  <Kbd className="h-4 px-1 text-[10px]">⌥</Kbd>
                  <Kbd className="h-4 px-1 text-[10px]">Space</Kbd>
                </KbdGroup>
                <span>push-to-talk</span>
                <Separator orientation="vertical" className="h-3" />
                <KbdGroup>
                  <Kbd className="h-4 px-1 text-[10px]">⇧</Kbd>
                  <Kbd className="h-4 px-1 text-[10px]">↵</Kbd>
                </KbdGroup>
                <span>newline</span>
                <span className="ml-auto font-mono">
                  7 listening · 2 transmitting
                </span>
              </div>
            </div>
          </main>
        </div>

        {/* ---------------------------------------------------- status bar */}
        <div className="flex h-[26px] shrink-0 items-center gap-2.5 overflow-hidden border-t bg-card px-3 text-[11px] whitespace-nowrap">
          <span className="flex shrink-0 items-center gap-1.5">
            <span className="size-1.5 rounded-full bg-emerald-400 ring-2 ring-emerald-400/25" />
            <span className="text-muted-foreground">
              Connected —{" "}
              <span className="font-mono text-foreground/80">
                voice.example.com
              </span>{" "}
              · <span className="font-mono text-foreground/80">48 ms</span>
            </span>
          </span>
          <Separator orientation="vertical" className="h-3.5 shrink-0" />
          <span className="flex shrink-0 items-center gap-1 text-muted-foreground">
            <Lock className="size-3 text-emerald-400/80" />
            <span className="font-mono">TLS 1.3</span>
          </span>
          <Separator orientation="vertical" className="h-3.5 shrink-0" />
          <span className="shrink-0 font-mono text-muted-foreground">
            UDP · 12 ms jitter
          </span>

          <div className="ml-auto flex shrink-0 items-center gap-2.5">
            <Meter
              label="IN"
              value={62}
              tone="[&>[data-slot=progress-indicator]]:bg-emerald-400"
            />
            <Meter
              label="OUT"
              value={38}
              tone="[&>[data-slot=progress-indicator]]:bg-sky-400"
            />
            <Separator orientation="vertical" className="h-3.5 shrink-0" />
            <StatusChip>
              <AudioLines className="size-2.5" />
              Opus · 96 kbit/s
            </StatusChip>
            <StatusChip>
              <Cpu className="size-2.5" />
              20 ms frames
            </StatusChip>
            <StatusChip>positional: off</StatusChip>
          </div>
        </div>
      </div>
    </TooltipProvider>
  )
}
