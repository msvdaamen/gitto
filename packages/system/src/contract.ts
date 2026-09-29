import { oc } from "@orpc/contract";
import * as z from "zod";

export const systemContract = {
  hello: oc.input(z.object({ name: z.string() })).output(z.object({ message: z.string() })),
};
