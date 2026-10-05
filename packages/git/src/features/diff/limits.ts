/**
 * The size of the largest file `getBlob` and `readWorkingTreeFile` read. A change of a line or two
 * can be in a file of any size, a generated one say, and the whole of it is sent to the renderer,
 * kept there and highlighted, which takes seconds for half a megabyte already.
 */
export const MAX_BLOB_BYTES = 5 * 1024 * 1024;

/**
 * The size of the largest patch sent to the renderer, which parses and highlights all of it. The UI
 * asks before showing a change of many lines, but can't tell how many there are in a file without
 * line counts, like an untracked one: a log of hundreds of megabytes, say.
 */
export const MAX_PATCH_BYTES = 10 * 1024 * 1024;
