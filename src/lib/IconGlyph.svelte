<script lang="ts">
  import {
    Archive,
    Bell,
    BookOpen,
    Bookmark,
    Brain,
    BriefcaseBusiness,
    CalendarDays,
    Camera,
    Car,
    CheckSquare,
    Code2,
    Cpu,
    FileText,
    Folder,
    Gift,
    Heart,
    Home,
    Inbox,
    List,
    Lock,
    Music,
    Notebook,
    Palette,
    Plane,
    Rocket,
    ShoppingCart,
    Star,
    Sun,
    Tag,
    Wrench
  } from "@lucide/svelte";

  export let icon = "list";
  export let size = 22;
  export let strokeWidth = 1.8;

  $: isEmoji = !knownIcons.has(icon);

  const knownIcons = new Set([
    "archive",
    "bell",
    "book",
    "bookmark",
    "brain",
    "briefcase",
    "calendar",
    "camera",
    "car",
    "check-square",
    "code",
    "cpu",
    "file",
    "folder",
    "gift",
    "heart",
    "home",
    "inbox",
    "list",
    "lock",
    "music",
    "notebook",
    "palette",
    "plane",
    "rocket",
    "shopping-cart",
    "star",
    "sun",
    "tag",
    "wrench"
  ]);
</script>

{#if isEmoji}
  <span class="emoji-glyph" style={`font-size: ${size}px`}>{icon}</span>
{:else if icon === "sun"}
  <Sun {size} {strokeWidth} />
{:else if icon === "calendar"}
  <CalendarDays {size} {strokeWidth} />
{:else if icon === "star"}
  <Star {size} {strokeWidth} />
{:else if icon === "notebook"}
  <Notebook {size} {strokeWidth} />
{:else if icon === "bookmark"}
  <Bookmark {size} {strokeWidth} />
{:else if icon === "bell"}
  <Bell {size} {strokeWidth} />
{:else if icon === "brain"}
  <Brain {size} {strokeWidth} />
{:else if icon === "briefcase"}
  <BriefcaseBusiness {size} {strokeWidth} />
{:else if icon === "camera"}
  <Camera {size} {strokeWidth} />
{:else if icon === "car"}
  <Car {size} {strokeWidth} />
{:else if icon === "check-square"}
  <CheckSquare {size} {strokeWidth} />
{:else if icon === "cpu"}
  <Cpu {size} {strokeWidth} />
{:else if icon === "palette"}
  <Palette {size} {strokeWidth} />
{:else if icon === "plane"}
  <Plane {size} {strokeWidth} />
{:else if icon === "rocket"}
  <Rocket {size} {strokeWidth} />
{:else if icon === "shopping-cart"}
  <ShoppingCart {size} {strokeWidth} />
{:else if icon === "wrench"}
  <Wrench {size} {strokeWidth} />
{:else if icon === "inbox"}
  <Inbox {size} {strokeWidth} />
{:else if icon === "home"}
  <Home {size} {strokeWidth} />
{:else if icon === "book"}
  <BookOpen {size} {strokeWidth} />
{:else if icon === "file"}
  <FileText {size} {strokeWidth} />
{:else if icon === "code"}
  <Code2 {size} {strokeWidth} />
{:else if icon === "gift"}
  <Gift {size} {strokeWidth} />
{:else if icon === "lock"}
  <Lock {size} {strokeWidth} />
{:else if icon === "heart"}
  <Heart {size} {strokeWidth} />
{:else if icon === "folder"}
  <Folder {size} {strokeWidth} />
{:else if icon === "tag"}
  <Tag {size} {strokeWidth} />
{:else if icon === "music"}
  <Music {size} {strokeWidth} />
{:else if icon === "archive"}
  <Archive {size} {strokeWidth} />
{:else}
  <List {size} {strokeWidth} />
{/if}
