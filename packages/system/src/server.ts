import { implement } from "@orpc/server";

import { systemContract } from "./contract";

const os = implement(systemContract);

export const systemRouter = os.router({
  hello: os.hello.handler(({ input }) => {
    return { message: `Hello, ${input.name}!` };
  }),
});
