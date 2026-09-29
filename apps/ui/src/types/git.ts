export type AppView = "home" | "repository";

export interface RepositoryTab {
  id: string;
  view: AppView;
  repositoryId?: string;
  title: string;
  dirty?: boolean;
}

export interface GitStatusCounts {
  modified: number;
  added: number;
  deleted: number;
}

export interface RepositorySummary {
  id: string;
  name: string;
  owner: string;
  path: string;
  provider: "GitHub" | "GitLab" | "Local";
  branch: string;
  language: string;
  languageColor: string;
  status: GitStatusCounts;
  pinned: boolean;
  lastOpened: string;
  description: string;
}

export interface Branch {
  name: string;
  current?: boolean;
  ahead?: number;
  behind?: number;
  remote?: boolean;
}

export type FileStatus = "modified" | "added" | "deleted" | "renamed";

export interface ChangedFile {
  path: string;
  status: FileStatus;
  additions: number;
  deletions: number;
}

export interface Commit {
  id: string;
  sha: string;
  graph: string[];
  message: string;
  description?: string;
  author: string;
  initials: string;
  avatarColor: string;
  timestamp: string;
  refs: string[];
  files: ChangedFile[];
  additions: number;
  deletions: number;
  isWip?: boolean;
}

export interface ActivityEntry {
  id: string;
  action: string;
  repository: string;
  branch: string;
  time: string;
  tone: "purple" | "blue" | "mint" | "amber";
}
