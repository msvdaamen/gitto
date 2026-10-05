/**
 * The size of the largest file `getBlob` and `readWorkingTreeFile` read. A change of a line or two
 * can be in a file of any size, a generated one say, and the whole of it is sent to the renderer,
 * kept there and highlighted, which takes seconds for half a megabyte already.
 */
export const MAX_BLOB_BYTES = 5 * 1024 * 1024;
