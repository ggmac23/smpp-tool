/**
 * The renderer has no Node globals, but the shared codec is written against
 * Buffer. esbuild injects this file into the browser bundle so `Buffer`
 * resolves to the `buffer` package's implementation instead of being
 * undefined at runtime.
 */
import { Buffer } from "buffer";

export { Buffer };
