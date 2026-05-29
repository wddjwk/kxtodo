export type NodeKind = "system" | "category" | "entry";

export type AppNode = {
  id: string;
  kind: NodeKind;
  name: string;
  icon: string;
  parentId: string | null;
  collapsed?: boolean;
  createdAt: string;
};

export type Task = {
  id: string;
  nodeId: string;
  markdown: string;
  completed: boolean;
  important: boolean;
  myDay: boolean;
  plannedDate?: string;
  dueDate?: string;
  expanded?: boolean;
  editing?: boolean;
  createdAt: string;
  updatedAt?: string;
};

export type ProfileSettings = {
  displayName: string;
  email: string;
  avatar: string;
};

export type Settings = {
  profile: ProfileSettings;
  appearance: {
    linkOpenMode: "app" | "system";
    uiScale: number;
  };
  lifecycle: {
    closeToTray: boolean;
    launchAtStartup: boolean;
  };
  shortcuts: {
    newTask: string;
    focusSearch: string;
    toggleWindow: string;
    openSettings: string;
  };
  cloud: {
    provider: "none" | "webdav" | "s3" | "custom";
    endpoint: string;
    enabled: boolean;
  };
};

export type ListBackground = {
  color: string;
  image?: string;
  imageOpacity?: number;
};

export type AppState = {
  schemaVersion: number;
  nodes: AppNode[];
  tasks: Task[];
  selectedNodeId: string;
  backgrounds: Record<string, ListBackground>;
};
