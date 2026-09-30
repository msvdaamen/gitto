export type { ChangedFile, FileStatus } from "@gitto/git/types";

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

/** A row in the history table: a commit, or the uncommitted changes in the working directory. */
export interface Commit {
  /** The repository the commit is in. */
  repositoryId: string;
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
