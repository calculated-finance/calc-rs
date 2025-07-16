import { getOrder, KEEPER_ADDRESS, PAIR_ADDRESS } from "./script";

// queryPool().then((pool) => console.log(JSON.stringify(pool, null, 2)));
getOrder(PAIR_ADDRESS, KEEPER_ADDRESS, "base", "1.00").then((order) =>
  console.log(JSON.stringify(order, null, 2)),
);
