import type { Ref } from "@gitto/git/types";

export type RefTreeNode = RefFolder | RefLeaf;

export type RefFolder = {
  type: "folder";
  name: string;
  /** Full ref prefix of the folder, e.g. `refs/remotes/origin/feature`. Unique across kinds. */
  path: string;
  /** Number of refs anywhere below this folder. */
  count: number;
  children: RefTreeNode[];
};

export type RefLeaf = {
  type: "ref";
  /** Last path segment of the ref's short name, e.g. `login` for `feature/login`. */
  name: string;
  ref: Ref;
};

/**
 * Groups refs into folders on `/`, e.g. `feature/login` → folder `feature` with ref `login`, and
 * `origin/feature/login` → `origin` › `feature` › `login`. Folders come before refs at each level;
 * otherwise the input order is kept.
 */
export function buildRefTree(refs: Ref[]): RefTreeNode[] {
  const root: RefFolder = { type: "folder", name: "", path: "", count: 0, children: [] };
  // By path, so a ref's folders are looked up rather than searched for among their siblings, of
  // which a remote can have thousands.
  const folders = new Map<string, RefFolder>();

  for (const ref of refs) {
    const segments = ref.name.split("/");
    const name = segments.pop() ?? ref.name;
    let path = ref.fullName.slice(0, ref.fullName.length - ref.name.length).replace(/\/$/, "");
    let folder = root;
    folder.count++;

    for (const segment of segments) {
      path = `${path}/${segment}`;
      let child = folders.get(path);
      if (!child) {
        child = { type: "folder", name: segment, path, count: 0, children: [] };
        folders.set(path, child);
        folder.children.push(child);
      }
      child.count++;
      folder = child;
    }

    folder.children.push({ type: "ref", name, ref });
  }

  return sortFoldersFirst(root.children);
}

function sortFoldersFirst(nodes: RefTreeNode[]): RefTreeNode[] {
  const folders = nodes.filter((node): node is RefFolder => node.type === "folder");
  const leaves = nodes.filter((node) => node.type === "ref");
  for (const folder of folders) folder.children = sortFoldersFirst(folder.children);
  return [...folders, ...leaves];
}

/** A line of the tree as the sidebar shows it: a folder or ref, indented `depth` levels. */
export type RefTreeRow = { node: RefTreeNode; depth: number };

/**
 * The tree as the flat list of lines it shows, top to bottom; the children of collapsed folders
 * (`isCollapsed` takes a folder's `path`) are left out. Flat so the sidebar can render only the
 * lines in view.
 */
export function flattenRefTree(
  nodes: RefTreeNode[],
  isCollapsed: (path: string) => boolean,
): RefTreeRow[] {
  const rows: RefTreeRow[] = [];
  const visit = (level: RefTreeNode[], depth: number) => {
    for (const node of level) {
      rows.push({ node, depth });
      if (node.type === "folder" && !isCollapsed(node.path)) visit(node.children, depth + 1);
    }
  };
  visit(nodes, 0);
  return rows;
}
