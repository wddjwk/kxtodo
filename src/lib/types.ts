export type ListKind = "system" | "custom";

export interface ListTheme {
  accent: string;
  background: string;
  image?: string;
  imageOpacity?: number;
}

export type ListNodeType = "category" | "entry";

export interface TodoList {
  id: string;
  parentId: string | null;
  kind: ListKind;
  nodeType: ListNodeType;
  name: string;
  icon: string;
  collapsed: boolean;
  shared: boolean;
  order: number;
  theme: ListTheme;
}

export interface TodoStep {
  id: string;
  title: string;
  completed: boolean;
}

export interface TodoTask {
  id: string;
  listId: string;
  markdown: string;
  completed: boolean;
  important: boolean;
  expanded: boolean;
  steps: TodoStep[];
  notes: string;
  dueDate: string | null;
  reminder: string | null;
  repeat: string | null;
  tags: string[];
  createdAt: string;
  updatedAt: string;
}

export type TodoTaskPatch = Partial<
  Pick<
    TodoTask,
    | "markdown"
    | "completed"
    | "important"
    | "expanded"
    | "steps"
    | "notes"
    | "dueDate"
    | "reminder"
    | "repeat"
    | "tags"
  >
>;

export interface AppState {
  schemaVersion: number;
  selectedListId: string;
  lists: TodoList[];
  tasks: TodoTask[];
  updatedAt: string;
}

export interface ShortcutBinding {
  id: string;
  label: string;
  combo: string;
}

export interface CloudSyncSettings {
  enabled: boolean;
  provider: "none" | "webdav" | "s3" | "custom";
  endpoint: string;
  status: "not_configured" | "disabled" | "ready";
}

export interface ProfileSettings {
  avatar: string;
  name: string;
  email: string;
}

export interface AppSettings {
  schemaVersion: number;
  profile: ProfileSettings;
  shortcuts: ShortcutBinding[];
  globalShortcut: string;
  sidebarWidth: number;
  taskDensity: "comfortable" | "compact";
  defaultListTheme: ListTheme;
  cloudSync: CloudSyncSettings;
}

export interface ExportPayload {
  schemaVersion: number;
  exportedAt: string;
  scope: "all" | "list";
  state?: AppState;
  lists?: TodoList[];
  tasks?: TodoTask[];
  rootListId?: string;
}
