import { queryPool } from "./script";

queryPool().then((pool) => console.log(JSON.stringify(pool, null, 2)));
