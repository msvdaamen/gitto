import { systemRouter, type SystemContext } from "@gitto/system/server";
import { implement } from "@orpc/server";

import { mainContract } from "./contract";
import { createRpcHandler } from "./handler";

export type MainContext = SystemContext;

const router = implement(mainContract).$context<MainContext>().router({
  system: systemRouter,
});

/** The main process's side of the API (see `mainContract`). */
export const createMainRpcHandler = () => createRpcHandler(router);
