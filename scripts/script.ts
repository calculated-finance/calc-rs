import { CodeDetails, SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { stringToPath } from "@cosmjs/crypto";
import {
  DirectSecp256k1HdWallet,
  DirectSecp256k1Wallet,
  type Coin,
} from "@cosmjs/proto-signing";
import { GasPrice, StargateClient } from "@cosmjs/stargate";
import { base64 } from "@scure/base";
import { bech32 } from "bech32";
import { config } from "dotenv";
import fs from "fs";
import protobuf from "protobufjs";
import { setTimeout } from "timers/promises";
import {
  ManagerExecuteMsg,
  SchedulerQueryMsg,
  StrategyExecuteMsg,
} from "../calc";
import types from "./MsgCompiled";

(BigInt.prototype as any).toJSON = function () {
  return this.toString();
};

config();

export const getWalletWithMnemonic = async () =>
  DirectSecp256k1HdWallet.fromMnemonic(process.env.MNEMONIC!, {
    prefix: process.env.PREFIX! || "sthor",
    hdPaths: [stringToPath(`m/44'/931'/0'/0/0`)],
  });

export const getWalletWithPrivateKey = async () =>
  DirectSecp256k1Wallet.fromKey(
    Buffer.from(process.env.PRIVATE_KEY, "hex"),
    process.env.PREFIX || "sthor",
  );

export const getSigner = async () => {
  console.log("Connecting to RPC URL:", process.env.RPC_URL);
  const signer = await SigningCosmWasmClient.connectWithSigner(
    process.env.RPC_URL!,
    await getWalletWithMnemonic(),
    // await getWalletWithPrivateKey(),
    {
      gasPrice: GasPrice.fromString(process.env.GAS_PRICE || "0.0urune"),
    },
  );

  signer.registry.register("/types.MsgDeposit", types.types.MsgDeposit);

  return signer;
};

export const upload = async (binaryFilePath: string) => {
  const wallet = await getWalletWithMnemonic();
  const cosmWasmClient = await getSigner();
  const adminAddress = await getAccount(wallet);

  const { codeId } = await cosmWasmClient.upload(
    adminAddress,
    fs.readFileSync(binaryFilePath),
    1.5,
  );

  return codeId;
};

export const uploadAndInstantiate = async (
  binaryFilePath: string,
  adminAddress: string,
  initMsg: Record<string, unknown>,
  label: string,
  funds: Coin[] = [],
): Promise<string> => {
  const cosmWasmClient = await getSigner();

  const { codeId } = await cosmWasmClient.upload(
    adminAddress,
    fs.readFileSync(binaryFilePath),
    1.5,
  );

  console.log("Uploaded code id:", codeId);

  const { contractAddress } = await cosmWasmClient.instantiate(
    adminAddress,
    codeId,
    initMsg,
    label,
    1.5,
    { funds, admin: adminAddress },
  );

  console.log(label, "contract address:", contractAddress);

  return contractAddress;
};

export const uploadAndMigrate = async (
  binaryFilePath: string,
  adminAddress: string,
  contractAddress: string,
  msg: Record<string, unknown> = {},
): Promise<void> => {
  const cosmWasmClient = await getSigner();
  const { codeId } = await cosmWasmClient.upload(
    adminAddress,
    fs.readFileSync(binaryFilePath),
    1.5,
  );

  console.log("Uploaded code id:", codeId);

  await cosmWasmClient.migrate(
    adminAddress,
    contractAddress,
    codeId,
    msg,
    "auto",
  );

  console.log("Migrated contract at address:", contractAddress);
};

export const getAccount = async (wallet: DirectSecp256k1HdWallet) => {
  const accounts = await wallet.getAccounts();
  return accounts[0]?.address;
};

export const uploadStrategyContract = async () => {
  return upload("artifacts/strategy.wasm");
};

export const uploadAndInstantiateManagerContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndInstantiate(
    "artifacts/manager.wasm",
    adminAddress,
    {
      fee_collector: adminAddress,
      strategy_code_id: await uploadStrategyContract(),
    },
    "Manager Contract",
  );
};

export const uploadAndInstantiateExchangeContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndInstantiate(
    "artifacts/exchanger.wasm",
    adminAddress,
    {},
    "Exchange Contract",
  );
};

export const uploadAndMigrateManagerContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/manager.wasm",
    adminAddress,
    MANAGER_ADDRESS,
    {
      fee_collector: adminAddress,
      strategy_code_id: await uploadStrategyContract(),
    },
  );
};

export const uploadAndMigrateStrategyContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/strategy.wasm",
    adminAddress,
    STRATEGY_ADDRESS,
    {
      fee_collector: adminAddress,
    },
  );
};

export const uploadAndMigrateExchangeContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/exchanger.wasm",
    adminAddress,
    EXCHANGE_ADDRESS,
    {
      scheduler_address: SCHEDULER_ADDRESS,
      affiliate_code: undefined,
      affiliate_bps: undefined,
    },
  );
};

export const uploadAndMigrateSchedulerContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/scheduler.wasm",
    adminAddress,
    SCHEDULER_ADDRESS,
  );
};

export const uploadAndInstantiateSchedulerContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndInstantiate(
    "artifacts/scheduler.wasm",
    adminAddress,
    {},
    "Scheduler Contract",
  );
};

export const getCodeDetails = async (codeId: number): Promise<CodeDetails> => {
  const cosmWasmClient = await getSigner();
  const info = await cosmWasmClient.getCodeDetails(codeId);

  return info;
};

export const uploadAndInstantiateContractSuite = async () => {
  await uploadAndInstantiateManagerContract();
  await uploadAndInstantiateExchangeContract();
  await uploadAndInstantiateSchedulerContract();
};

export const uploadAndMigrateContractSuite = async () => {
  await uploadAndMigrateManagerContract();
  await uploadAndMigrateExchangeContract();
  await uploadAndMigrateSchedulerContract();
};

export const uploadPairs = async () => {
  const cosmWasmClient = await getSigner();

  const account = await getAccount(await getWalletWithMnemonic());

  await cosmWasmClient.execute(
    account,
    SCHEDULER_ADDRESS,
    {
      create_pairs: {
        pairs: [{}],
      },
    },
    "auto",
  );
};

export const fetchBalances = async (address: string) => {
  const stargateClient = await StargateClient.connect(process.env.RPC_URL!);
  const balances = await stargateClient.getAllBalances(address);

  return balances;
};

export const canSwap = async () => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(EXCHANGE_ADDRESS, {
    can_swap: {
      swap_amount: {
        denom: "rune",
        amount: "100000000",
      },
      minimum_receive_amount: {
        denom: "x/ruji",
        amount: "49000",
      },
    },
  });

  return response;
};

export const getExpectedReceiveAmount = async (
  swapAmount: Coin,
  targetDenom: string,
  route: any,
) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(EXCHANGE_ADDRESS, {
    expected_receive_amount: {
      swap_amount: {
        denom: swapAmount.denom,
        amount: `${swapAmount.amount}`,
      },
      target_denom: targetDenom,
      route,
    },
  });

  return response;
};

export const getSpotPrice = async () => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(EXCHANGE_ADDRESS, {
    spot_price: {
      swap_denom: "rune",
      target_denom: "x/ruji",
      period: 0,
    },
  });

  return response;
};

export const getRoute = async () => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(EXCHANGE_ADDRESS, {
    route: {
      swap_amount: {
        denom: "rune",
        amount: "100000000",
      },
      target_denom: "x/ruji",
    },
  });

  return response;
};

export const swap = async (
  swapAmount: Coin,
  targetDenom: string,
  route: any,
) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const response = await cosmWasmClient.execute(
    account,
    EXCHANGE_ADDRESS,
    {
      swap: {
        minimum_receive_amount: {
          denom: targetDenom,
          amount: "1",
        },
        maximum_slippage_bps: 100,
        route,
      },
    },
    "auto",
    "Swap",
    [swapAmount],
  );

  return response;
};

export const getConfig = async (contractAddress: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(contractAddress, {
    config: {},
  });

  return response;
};

export const bech32ToBase64 = (address: string): string =>
  base64.encode(
    Uint8Array.from(bech32.fromWords(bech32.decode(address).words)),
  );

export const executeDeposit = async (memo: string, funds: any[]) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());

  const response = await cosmWasmClient.signAndBroadcast(
    account,
    [
      {
        typeUrl: "/types.MsgDeposit",
        value: {
          signer: bech32ToBase64(account),
          memo,
          coins: funds,
        },
      },
    ],
    "auto",
    memo,
  );

  return response;
};

// const createStrategy = async () => {
//   const cosmWasmClient = await getSigner();
//   const account = await getAccount(await getWalletWithMnemonic());

//   const response = await cosmWasmClient.execute(
//     account,
//     MANAGER_ADDRESS,
//     {
//       instantiate_strategy: {
//         action: {
//           exhibit: {
//             threshold: "any",
//             actions: [
//               {
//                 exhibit: {
//                   threshold: "all",
//                   actions: [
//                     {
//                       crank: {
//                         cadence: {
//                           blocks: {
//                             interval: 10,
//                             previous: 0,
//                           },
//                         },
//                         execution_rebate: [],
//                         scheduler: SCHEDULER_ADDRESS,
//                       },
//                     },
//                     {
//                       perform: {
//                         adjustment: "fixed",
//                         exchange_contract: EXCHANGE_ADDRESS,
//                         maximum_slippage_bps: 200,
//                         minimum_receive_amount: {
//                           denom: "eth-usdt",
//                           amount: "1",
//                         },
//                         swap_amount: {
//                           denom: "rune",
//                           amount: "20000000",
//                         },
//                       },
//                     },
//                   ],
//                 },
//               },
//               {
//                 exhibit: {
//                   threshold: "all",
//                   actions: [
//                     {
//                       crank: {
//                         cadence: {
//                           blocks: {
//                             interval: 10,
//                             previous: 5,
//                           },
//                         },
//                         execution_rebate: [],
//                         scheduler: SCHEDULER_ADDRESS,
//                       },
//                     },
//                     {
//                       perform: {
//                         adjustment: "fixed",
//                         exchange_contract: EXCHANGE_ADDRESS,
//                         maximum_slippage_bps: 200,
//                         minimum_receive_amount: {
//                           denom: "rune",
//                           amount: "1",
//                         },
//                         swap_amount: {
//                           denom: "eth-usdt",
//                           amount: "20000000",
//                         },
//                       },
//                     },
//                   ],
//                 },
//               },
//             ],
//           },
//         },
//         affiliates: [],
//         label: "Test",
//         owner: account,
//       },
//     } as ManagerExecuteMsg,
//     "auto",
//     "Create Strategy",
//     [
//       // {
//       //   denom: "rune",
//       //   amount: "200000000",
//       // },
//     ],
//   );

//   return response;
// };

export const getStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(MANAGER_ADDRESS, {
    strategy: {
      address,
    },
  });

  return response;
};

export const getStrategyConfig = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(address, {
    config: {},
  });

  return response;
};

export const getStrategies = async () => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(MANAGER_ADDRESS, {
    strategies: {},
  });

  return response;
};

export const executeStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      execute_strategy: {
        contract_address: address,
      },
    },
    "auto",
  );

  return response;
};

export const getTimeTriggers = async () => {
  const cosmWasmClient = await getSigner();
  const triggers = await cosmWasmClient.queryContractSmart(SCHEDULER_ADDRESS, {
    filtered: {
      limit: 10,
      filter: {
        timestamp: {
          start: undefined,
          end: undefined,
        },
      },
    },
  } as SchedulerQueryMsg);

  return triggers;
};

export const getBlockTriggers = async () => {
  const cosmWasmClient = await getSigner();

  const block = await cosmWasmClient.getBlock();

  const triggers = await cosmWasmClient.queryContractSmart(SCHEDULER_ADDRESS, {
    filtered: {
      limit: 10,
      filter: {
        block_height: {
          start: undefined,
          end: undefined,
        },
      },
    },
  } as SchedulerQueryMsg);

  return triggers;
};

export const getAllTriggers = async () => {
  return [...(await getBlockTriggers()), ...(await getTimeTriggers())];
};

export const getOwnedTriggers = async (owner: string) => {
  const cosmWasmClient = await getSigner();
  const triggers = await cosmWasmClient.queryContractSmart(SCHEDULER_ADDRESS, {
    owned: {
      owner,
    },
  } as SchedulerQueryMsg);

  return triggers;
};

export const executeTriggersWith = async (
  getTriggers: () => Promise<any[]>,
) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const triggers = await getTriggers();

  console.log("Triggers to execute:", triggers);

  for (const trigger of triggers) {
    const response = await cosmWasmClient.execute(
      account,
      SCHEDULER_ADDRESS,
      { execute_trigger: trigger.id },
      "auto",
    );

    console.log("Executed trigger:", trigger.id, JSON.stringify(response));
  }
};

export const executeProvidedTriggers = async (triggers: any[]) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());

  console.log("Provided triggers to execute:", triggers);

  for (const trigger of triggers) {
    try {
      const response = await cosmWasmClient.execute(
        account,
        SCHEDULER_ADDRESS,
        { execute_trigger: trigger.id },
        "auto",
      );

      console.log("Executed trigger:", trigger.id, response);
    } catch (error) {
      console.error("Error executing trigger:", trigger.id, error);
    }
  }
};

export const executeTriggers = async (owner: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());

  const triggers = await cosmWasmClient.queryContractSmart(SCHEDULER_ADDRESS, {
    triggers: {
      filter: {
        owner: {
          address: owner,
        },
      },
      limit: 10,
      can_execute: true,
    },
  });

  console.log("Triggers to execute:", triggers);

  for (const { id } of triggers) {
    const response = await cosmWasmClient.execute(
      account,
      SCHEDULER_ADDRESS,
      { execute_trigger: id },
      "auto",
    );

    return response;
  }
};

export const resumeStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      resume_strategy: {
        contract_address: address,
      },
    },
    "auto",
  );

  return response;
};

export const withdrawFromStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const balances = await fetchBalances(address);
  const response = await cosmWasmClient.execute(
    account,
    address,
    {
      withdraw: balances.map((b) => b.denom),
    } as StrategyExecuteMsg,
    "auto",
  );

  return response;
};

export const queryContract = async (
  contractAddress: string,
  msg: Record<string, unknown>,
) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(
    contractAddress,
    msg,
  );

  return response;
};

export const executeTxn = async (
  contractAddress: string,
  msg: Record<string, unknown>,
  funds: Coin[] = [],
) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const response = await cosmWasmClient.execute(
    account,
    contractAddress,
    msg,
    "auto",
    undefined,
    funds,
  );

  return response;
};

export const getMyBalances = async () => {
  return fetchBalances(await getAccount(await getWalletWithMnemonic()));
};

export const fetchFinBook = async (pairAddress: string) => {
  const cosmWasmClient = await getSigner();
  const book = await cosmWasmClient.queryContractSmart(pairAddress, {
    book: {
      limit: 10,
    },
  });

  return book;
};

export const getStatistics = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(address, {
    statistics: {},
  });

  return response;
};

export const getTransaction = async (txHash: string) => {
  const stargateClient = await StargateClient.connect(process.env.RPC_URL!);
  const tx = await stargateClient.getTx(txHash);

  return tx;
};

export const getSwapQuote = async ({
  swapAmount,
  targetDenom,
  recipient,
  affiliateCode,
  affiliateBps,
}: {
  swapAmount: Coin;
  targetDenom: string;
  recipient?: string;
  affiliateCode?: string;
  affiliateBps?: number;
}) => {
  const response = await fetch(
    `https://stagenet-thornode.ninerealms.com/thorchain/quote/swap?from_asset=${swapAmount.denom}&to_asset=${targetDenom}&amount=${swapAmount.amount}&destination=${recipient}`,
  );

  return response.json();
};

export const queryPool = async () => {
  const stargateClient = await getSigner();
  const root = await protobuf.load("./scripts/query.proto");

  const QueryPoolRequest = root.lookupType("types.QueryPoolRequest");
  const QueryPoolResponse = root.lookupType("types.QueryPoolResponse");

  const request = QueryPoolRequest.encode({
    asset: "eth.eth",
    height: "0",
  }).finish();

  const response = await stargateClient["getQueryClient"]().queryAbci(
    "/types.Query/Pool",
    request,
  );

  return QueryPoolResponse.decode(response.value).toJSON();
};

queryPool().then((pool) => console.log(JSON.stringify(pool, null, 2)));

export const queryQuote = async () => {
  const stargateClient = await getSigner();
  const root = await protobuf.load("./scripts/query.proto");

  const QueryQuoteRequest = root.lookupType("types.QueryQuoteSwapRequest");
  const QueryQuoteResponse = root.lookupType("types.QueryQuoteSwapResponse");

  const request = QueryQuoteRequest.encode({
    fromAsset: "RUNE",
    toAsset: "ETH-USDT",
    amount: "15000000",
    destination: "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka",
    tolerance_bps: 100,
  }).finish();

  const response = await stargateClient["getQueryClient"]().queryAbci(
    "/types.Query/QuoteSwap",
    request,
  );

  return QueryQuoteResponse.decode(response.value).toJSON();
};

export const updateStrategy = async (address: string, update: any) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());

  const existingConfig = await cosmWasmClient.queryContractSmart(
    STRATEGY_ADDRESS,
    {
      config: {},
    },
  );

  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      update_strategy: {
        contract_address: STRATEGY_ADDRESS,
        update,
      },
    } as ManagerExecuteMsg,
    // update,
    "auto",
  );

  return response;
};

export const bankSend = async (amount: Coin, recipient: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());

  const response = await cosmWasmClient.sendTokens(
    account,
    recipient,
    [amount],
    "auto",
  );

  return response;
};

export const run = async () => {
  let triggers = await getAllTriggers();
  while (true) {
    await executeProvidedTriggers(triggers);
    await setTimeout(10_000);
    triggers = await getAllTriggers();
  }
};

export const setOrders = async (
  pairAddress: string,
  orders: any[],
  funds: Coin[],
) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());

  const response = await cosmWasmClient.execute(
    account,
    pairAddress,
    {
      order: orders,
    },
    "auto",
    "",
    funds,
  );

  return response;
};

export const getOrders = async (pairAddress: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const response = await cosmWasmClient.queryContractSmart(pairAddress, {
    orders: {
      limit: 10,
      owner: account,
    },
  });

  return response;
};

export const getBlock = async () => {
  const cosmWasmClient = await getSigner();
  return cosmWasmClient.getBlock();
};

export const MANAGER_ADDRESS =
  "sthor1xg6qsvyktr0zyyck3d67mgae0zun4lhwwn3v9pqkl5pk8mvkxsnscenkc0";

export const EXCHANGE_ADDRESS =
  "sthor196c0zhmpaktqu3hfgdafvsdlr3x9tz0n78qvwn7g7g2c7zmaa0jqxcd6st";

export const SCHEDULER_ADDRESS =
  "sthor1x3hfzl0v43upegeszz8cjygljgex9jtygpx4l44nkxudxjsukn3setrkl6";

export const STRATEGY_ADDRESS =
  "sthor17rkr38lk6vxcnw9ywyu64jjymny0yf42h4c4vj2hhm6chrt44heqtvnnmu";

export const PAIR_ADDRESS =
  "sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r";

// uploadContractSuite();
// fetchBalances(STRATEGY_ADDRESS).then(console.log);
// getMyBalances().then(console.log);
// bankSend(
//   {
//     amount: "153236136",
//     denom: "x/ruji",
//   },
//   STRATEGY_ADDRESS
// ).then(console.log);
// fetchFinBook(PAIR_ADDRESS);
// updateStrategy(STRATEGY_ADDRESS, {
//   manager: "sthor1xg6qsvyktr0zyyck3d67mgae0zun4lhwwn3v9pqkl5pk8mvkxsnscenkc0",
//   owner: "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka",
//   escrowed: ["rune", "eth-usdt"],
//   action: {
//     exhibit: {
//       actions: [
//         {
//           exhibit: {
//             actions: [
//               {
//                 crank: {
//                   scheduler: SCHEDULER_ADDRESS,
//                   cadence: {
//                     blocks: {
//                       interval: 10,
//                       previous: 4959840,
//                     },
//                   },
//                   execution_rebate: [],
//                 },
//               },
//               {
//                 perform: {
//                   exchange_contract:
//                     "sthor196c0zhmpaktqu3hfgdafvsdlr3x9tz0n78qvwn7g7g2c7zmaa0jqxcd6st",
//                   swap_amount: {
//                     denom: "rune",
//                     amount: "20000000",
//                   },
//                   minimum_receive_amount: {
//                     denom: "eth-usdt",
//                     amount: "1",
//                   },
//                   maximum_slippage_bps: 200,
//                   adjustment: "fixed",
//                   route: null,
//                 },
//               },
//             ],
//             threshold: "all",
//           },
//         },
//         {
//           exhibit: {
//             actions: [
//               {
//                 crank: {
//                   scheduler: SCHEDULER_ADDRESS,
//                   cadence: {
//                     blocks: {
//                       interval: 10,
//                       previous: 4959835,
//                     },
//                   },
//                   execution_rebate: [],
//                 },
//               },
//               {
//                 perform: {
//                   exchange_contract:
//                     "sthor196c0zhmpaktqu3hfgdafvsdlr3x9tz0n78qvwn7g7g2c7zmaa0jqxcd6st",
//                   swap_amount: {
//                     denom: "eth-usdt",
//                     amount: "20000000",
//                   },
//                   minimum_receive_amount: {
//                     denom: "rune",
//                     amount: "1",
//                   },
//                   maximum_slippage_bps: 200,
//                   adjustment: "fixed",
//                   route: null,
//                 },
//               },
//             ],
//             threshold: "all",
//           },
//         },
//       ],
//       threshold: "any",
//     },
//   },
// }).then(console.log);
// createStrategy().then((r) => console.log(JSON.stringify(r, null, 2)));
// getStrategy(STRATEGY_ADDRESS).then(console.log);
// getStrategies().then(console.log);
// getConfig(STRATEGY_ADDRESS).then((c) =>
//   console.log(JSON.stringify(c, null, 2)),
// );
// getStatistics(STRATEGY_ADDRESS).then((s) =>
//   console.log(JSON.stringify(s, null, 2)),
// );
// bankSend(
//   {
//     denom: "rune",
//     amount: "10000000",
//   },
//   STRATEGY_ADDRESS,
// ).then(console.log);
// executeTriggersWith(getBlockTriggers);
// executeTriggersWith(getTimeTriggers);
// run();
// getBlockTriggers().then(console.log);
// getAllTriggers().then((r) => console.log(JSON.stringify(r, null, 2)));
// getOwnedTriggers(STRATEGY_ADDRESS).then(async (r) => {
//   console.log(JSON.stringify(r, null, 2));
//   console.log(await getBlock());
// });
// executeStrategy(STRATEGY_ADDRESS).then((r) =>
//   console.log(JSON.stringify(r, null, 2)),
// );
// executeTriggers(STRATEGY_ADDRESS).then((result) => {
//   console.log("Trigger execution result:", result);
// getStatistics(STRATEGY_ADDRESS).then((c) =>
//   console.log(JSON.stringify(c, null, 2))
// );
// queryPool().then(console.log);
// queryQuote().then(console.log);
// getStatistics(STRATEGY_ADDRESS).then(console.log);
// getTransaction(
//   "E69D46C0C2CCC2B851E7456BE513A04A90C10D2B9857A858CCBF0779A385F30D",
// ).then((t) => console.log(JSON.stringify(t.events, null, 2)));
// withdrawFromStrategy(STRATEGY_ADDRESS);
// uploadAndMigrateTwapContract();
// uploadDistributorContract().then(console.log);
// uploadAndMigrateDistributorContract();
// uploadAndMigrateExchangeContract();
// uploadAndInstantiateSchedulerContract();
// uploadAndMigrateSchedulerContract();
// uploadAndMigrateManagerContract();
// resumeStrategy(STRATEGY_ADDRESS);
// uploadAndMigrateContractSuite();
// uploadContractSuite();
// getFinBook("sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r");
// canSwap();
const swapAmount = {
  denom: "gaia-atom",
  amount: "10000000000000",
};
const targetDenom = "rune";
// getExpectedReceiveAmount(swapAmount, targetDenom, {
//   thorchain: {
//     streaming_interval: 5,
//     max_streaming_quantity: 0,
//   },
// } as Route).then(console.log);
// getSwapQuote({
//   swapAmount,
//   targetDenom,
//   recipient: "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka",
// }).then(console.log);
// getRoute();
// getWallet().then((wallet) => getAccount(wallet).then(console.log));
// swap(swapAmount, targetDenom, {
//   fin: { address: PAIR_ADDRESS },
// }).then(console.log);
// queryContract(EXCHANGE_ADDRESS, {
//   custom: {},
// }).then(console.log);
// executeDeposit("=:THOR.RUNE:thor133q36r4sg4ws3h2z7xredrsvq76e8tmq9r23ex:1", [
//   {
//     amount: "1935463600",
//     asset: {
//       chain: "ETH",
//       symbol: "USDT",
//       ticker: "USDT",
//       synth: false,
//       trade: false,
//       secured: true,
//     },
//   },
// ]).then(console.log);
// uploadAndInstantiateExchangeContract();
// executeTxn(EXCHANGE_ADDRESS, {
//   withdraw: {
//     denoms: ["eth-usdt-0xdac17f958d2ee523a2206206994597c13d831ec7"],
//   },
// });
// fetchFinBook(PAIR_ADDRESS).then((book) =>
//   console.log(JSON.stringify(book, null, 2)),
// );
// getConfig(PAIR_ADDRESS).then((config) =>
//   console.log(JSON.stringify(config, null, 2)),
// );
// getCodeDetails(DISTRIBUTOR_CODE_ID).then((details) => console.log(details));
// setOrders(
//   PAIR_ADDRESS,
//   [
//     [
//       ["quote", { fixed: "0.38" }, "0"],
//       ["quote", { fixed: "0.38" }, "1000"],
//     ],
//     null,
//   ],
//   [
//     {
//       denom: "rune",
//       amount: "1000",
//     },
//   ],
// ).then((_) =>
//   getOrders(PAIR_ADDRESS).then((orders) =>
//     console.log(JSON.stringify(orders, null, 2)),
//   ),
// );

// fetchFinBook(PAIR_ADDRESS).then((book) =>
//   console.log(JSON.stringify(book, null, 2)),
// );
