import type { CommitRef } from "@gitto/git/types";

import type { GraphRow } from "@/git/graph";
import type { RefLabel } from "@/git/ref-labels";

export type { ChangedFile, CommitRef, FileStatus } from "@gitto/git/types";

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
  /** Its row in the history graph; only rows in the history table have one. */
  graph?: GraphRow;
  message: string;
  description?: string;
  author: string;
  /** The author's email address, which their profile picture is looked up by. */
  email?: string;
  initials: string;
  avatarColor: string;
  timestamp: string;
  refs: CommitRef[];
  /** `refs` merged into the labels the history shows (see `toRefLabels`). */
  labels: RefLabel[];
  isWip?: boolean;
}

export interface ActivityEntry {
  id: string;
  kind: "push" | "merge" | "branch";
  action: string;
  repository: string;
  branch: string;
  time: string;
  tone: "purple" | "blue" | "mint" | "amber";
}
