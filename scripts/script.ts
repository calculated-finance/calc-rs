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
import types from "./MsgCompiled";

config();

const MANAGER_ADDRESS =
  "sthor1xg6qsvyktr0zyyck3d67mgae0zun4lhwwn3v9pqkl5pk8mvkxsnscenkc0";

const EXCHANGE_ADDRESS =
  "sthor196c0zhmpaktqu3hfgdafvsdlr3x9tz0n78qvwn7g7g2c7zmaa0jqxcd6st";

const SCHEDULER_ADDRESS =
  "sthor1dvdcm5r08utc9axjhywuw3e8lq2q4tfnmxgjg7mtf2s8mtl959fqg8nr8v";

const STRATEGY_ADDRESS =
  "sthor13wx9rc53am928agavdch5ap0p3e6hhuvn033tdf2vlrh0h4yqkpqss8l9j";

const DISTRIBUTOR_ADDRESS =
  "sthor1xjkswqj9fwqvfxasugzxx5qg0duvh2lw77dwpxmntx0etx0mhd2ssgjnqw";

const PAIR_ADDRESS =
  "sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r";

const DISTRIBUTOR_CODE_ID = 415;

const getWalletWithMnemonic = async () =>
  DirectSecp256k1HdWallet.fromMnemonic(process.env.MNEMONIC!, {
    prefix: process.env.PREFIX! || "sthor",
    hdPaths: [stringToPath(`m/44'/931'/0'/0/0`)],
  });

const getWalletWithPrivateKey = async () =>
  DirectSecp256k1Wallet.fromKey(
    Buffer.from(process.env.PRIVATE_KEY, "hex"),
    process.env.PREFIX || "sthor",
  );

const getSigner = async () => {
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

const uploadTwapContract = async () => {
  return upload("artifacts/twap.wasm");
};

const uploadDistributorContract = async () => {
  return upload("artifacts/distributor.wasm");
};

const uploadAndInstantiateManagerContract = async (
  code_ids: [string, number][],
) => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndInstantiate(
    "artifacts/manager.wasm",
    adminAddress,
    {
      code_ids,
      fee_collector: adminAddress,
    },
    "Manager Contract",
  );
};

const uploadAndInstantiateExchangeContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndInstantiate(
    "artifacts/exchanger.wasm",
    adminAddress,
    {},
    "Exchange Contract",
  );
};

const uploadAndMigrateManagerContract = async (
  code_ids: [string, number][],
) => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/manager.wasm",
    adminAddress,
    MANAGER_ADDRESS,
    {
      code_ids,
      fee_collector: adminAddress,
    },
  );
};

const uploadAndMigrateTwapContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/twap.wasm",
    adminAddress,
    STRATEGY_ADDRESS,
    {
      fee_collector: adminAddress,
    },
  );
};

const uploadAndMigrateDistributorContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/distributor.wasm",
    adminAddress,
    DISTRIBUTOR_ADDRESS,
    {},
  );
};

const uploadAndMigrateExchangeContract = async () => {
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

const uploadAndMigrateSchedulerContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndMigrate(
    "artifacts/scheduler.wasm",
    adminAddress,
    SCHEDULER_ADDRESS,
  );
};

const uploadAndInstantiateSchedulerContract = async () => {
  const wallet = await getWalletWithMnemonic();
  const adminAddress = await getAccount(wallet);

  return uploadAndInstantiate(
    "artifacts/scheduler.wasm",
    adminAddress,
    {},
    "Scheduler Contract",
  );
};

const getCodeDetails = async (codeId: number): Promise<CodeDetails> => {
  const cosmWasmClient = await getSigner();
  const info = await cosmWasmClient.getCodeDetails(codeId);

  return info;
};

const uploadAndInstantiateContractSuite = async () => {
  const distributorCodeId = await uploadDistributorContract();
  console.log("Distributor code ID:", distributorCodeId);
  const strategyCodeId = await uploadTwapContract();
  const codeDetails = await getCodeDetails(strategyCodeId);
  await uploadAndInstantiateManagerContract([["twap", strategyCodeId]]);
  await uploadAndInstantiateExchangeContract();
  await uploadAndInstantiateSchedulerContract();
};

const uploadAndMigrateContractSuite = async () => {
  const strategyCodeId = await uploadTwapContract();
  await uploadAndMigrateManagerContract([["twap", strategyCodeId]]);
  await uploadAndMigrateExchangeContract();
  await uploadAndMigrateSchedulerContract();
};

const uploadPairs = async () => {
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

const fetchBalances = async (address: string) => {
  const stargateClient = await StargateClient.connect(process.env.RPC_URL!);
  const balances = await stargateClient.getAllBalances(address);

  return balances;
};

const canSwap = async () => {
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

const getExpectedReceiveAmount = async (
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

const getSpotPrice = async () => {
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

const getRoute = async () => {
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

const swap = async (swapAmount: Coin, targetDenom: string, route: any) => {
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

const getConfig = async (contractAddress: string) => {
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

const executeDeposit = async (memo: string, funds: any[]) => {
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

const createStrategy = async () => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());

  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      instantiate_strategy: {
        owner: account,
        label: "ATOM -> RUNE TWAP",
        strategy: {
          twap: {
            distributor_code_id: DISTRIBUTOR_CODE_ID,
            owner: account,
            swap_amount: {
              denom: "gaia-atom",
              amount: "10000000",
            },
            minimum_receive_amount: {
              denom: "rune",
              amount: "1",
            },
            maximum_slippage_bps: 10,
            swap_cadence: {
              blocks: {
                interval: 3,
              },
            },
            exchanger_contract: EXCHANGE_ADDRESS,
            scheduler_contract: SCHEDULER_ADDRESS,
            mutable_destinations: [
              {
                recipient: {
                  bank: {
                    address: account,
                  },
                },
                shares: "893232",
                label: "Me",
              },
            ],
            immutable_destinations: [
              {
                recipient: {
                  bank: {
                    address: account,
                  },
                },
                shares: "234657",
                label: "Other Me",
              },
            ],
          },
        },
      },
    },
    "auto",
    "Create Strategy",
    [
      {
        denom: "gaia-atom",
        amount: "100000000",
      },
    ],
  );

  return response;
};

const getStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(MANAGER_ADDRESS, {
    strategy: {
      address,
    },
  });

  return response;
};

const getStrategyConfig = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(address, {
    config: {},
  });

  return response;
};

const getStrategies = async () => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(MANAGER_ADDRESS, {
    strategies: {},
  });

  return response;
};

const executeStrategy = async (address: string) => {
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

const getTimeTriggers = async () => {
  const cosmWasmClient = await getSigner();
  const triggers = await cosmWasmClient.queryContractSmart(SCHEDULER_ADDRESS, {
    triggers: {
      limit: 10,
      filter: {
        timestamp: {
          start: undefined,
          end: `${new Date().getTime()}`,
        },
      },
    },
  });

  return triggers;
};

const getBlockTriggers = async () => {
  const cosmWasmClient = await getSigner();

  const block = await cosmWasmClient.getBlock();

  const triggers = await cosmWasmClient.queryContractSmart(SCHEDULER_ADDRESS, {
    triggers: {
      limit: 10,
      filter: {
        block_height: {
          start: undefined,
          end: block.header.height,
        },
      },
    },
  });

  return triggers;
};

const getAllTriggers = async () => {
  return [...(await getBlockTriggers()), ...(await getTimeTriggers())];
};

const executeTriggersWith = async (getTriggers: () => Promise<any[]>) => {
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

    console.log("Executed trigger:", trigger.id, response);
  }
};

const executeProvidedTriggers = async (triggers: any[]) => {
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

const executeTriggers = async (owner: string) => {
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

const resumeStrategy = async (address: string) => {
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

const withdrawFromStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWalletWithMnemonic());
  const balances = await fetchBalances(address);
  const response = await cosmWasmClient.execute(
    account,
    address,
    {
      withdraw: {
        amounts: balances,
      },
    },
    "auto",
  );

  return response;
};

const queryContract = async (
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

const executeTxn = async (
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

const getMyBalances = async () => {
  return fetchBalances(await getAccount(await getWalletWithMnemonic()));
};

const fetchFinBook = async (pairAddress: string) => {
  const cosmWasmClient = await getSigner();
  const book = await cosmWasmClient.queryContractSmart(pairAddress, {
    book: {
      limit: 10,
    },
  });

  return book;
};

const getStatistics = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(address, {
    statistics: {},
  });

  return response;
};

const getTransaction = async (txHash: string) => {
  const stargateClient = await StargateClient.connect(process.env.RPC_URL!);
  const tx = await stargateClient.getTx(txHash);

  return tx;
};

const getSwapQuote = async ({
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

const queryPool = async () => {
  const stargateClient = await getSigner();
  const root = await protobuf.load("./scripts/query.proto");

  const QueryPoolRequest = root.lookupType("types.QueryPoolRequest");
  const QueryPoolResponse = root.lookupType("types.QueryPoolResponse");

  const request = QueryPoolRequest.encode({
    asset: "eth.usdt-0xdac17f958d2ee523a2206206994597c13d831ec7",
    height: "0",
  }).finish();

  const response = await stargateClient["getQueryClient"]().queryAbci(
    "/types.Query/Pool",
    request,
  );

  return QueryPoolResponse.decode(response.value).toJSON();
};

const queryQuote = async () => {
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

const updateStrategy = async (updates: Record<string, unknown>) => {
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
    STRATEGY_ADDRESS,
    {
      update: {
        twap: {
          ...existingConfig,
          ...updates,
        },
      },
    },
    "auto",
  );

  return response;
};

const bankSend = async (amount: Coin, recipient: string) => {
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

const run = async () => {
  let triggers = await getAllTriggers();
  while (true) {
    await executeProvidedTriggers(triggers);
    await setTimeout(10_000);
    triggers = await getAllTriggers();
  }
};

// uploadContractSuite();
// fetchBalances("thor133q36r4sg4ws3h2z7xredrsvq76e8tmq9r23ex").then(console.log);
getMyBalances().then(console.log);
// bankSend(
//   {
//     amount: "153236136",
//     denom: "x/ruji",
//   },
//   STRATEGY_ADDRESS
// ).then(console.log);
// fetchFinBook(PAIR_ADDRESS);
// updateStrategy({
//   route: {
//     fin: {
//       address: PAIR_ADDRESS,
//     },
//   },
// }).then(console.log);
// createStrategy().then(run);
// getStrategy(STRATEGY_ADDRESS);
// getStrategies().then(console.log);
// getStrategies();
// getConfig(STRATEGY_ADDRESS).then((c) =>
//   console.log(JSON.stringify(c, null, 2))
// );
// getStatistics(STRATEGY_ADDRESS).then((s) =>
//   console.log(JSON.stringify(s, null, 2))
// );
// executeTriggersWith(getBlockTriggers);
// executeTriggersWith(getTimeTriggers);
// run();
// getBlockTriggers().then(console.log);
// getAllTriggers().then(console.log);
// executeStrategy(STRATEGY_ADDRESS);
// executeTriggers(STRATEGY_ADDRESS).then((result) => {
//   console.log("Trigger execution result:", result);
// getStatistics(STRATEGY_ADDRESS).then((c) =>
//   console.log(JSON.stringify(c, null, 2))
// );
// });
// queryPool().then(console.log);
// queryQuote().then(console.log);
// getStatistics(STRATEGY_ADDRESS).then(console.log);
// getTransaction(
//   "5A54E3F51A2DB27BDAA857914EBA49CE9309BE07C4D66AA8A10F1A6FE97962B9"
// ).then((t) => console.log(JSON.stringify(t.events, null, 2)));
// withdrawFromStrategy(DISTRIBUTOR_ADDRESS);
// uploadAndMigrateTwapContract();
// uploadDistributorContract().then(console.log);
// uploadAndMigrateDistributorContract();
// uploadAndMigrateExchangeContract();
// uploadAndMigrateSchedulerContract();
// uploadAndMigrateManagerContract();
// resumeStrategy(STRATEGY_ADDRESS);
// uploadAndMigrateContractSuite();
// uploadContractSuite();
// getFinBook("sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r");
// canSwap();
const swapAmount = {
  denom: "gaia-atom",
  amount: "10000000",
};
const targetDenom = "rune";
// getExpectedReceiveAmount(swapAmount, targetDenom, {
//   fin: { address: PAIR_ADDRESS },
// }).then(console.log);
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
executeDeposit("=:THOR.RUNE:thor133q36r4sg4ws3h2z7xredrsvq76e8tmq9r23ex:1", [
  {
    amount: "1935463600",
    asset: {
      chain: "ETH",
      symbol: "USDT",
      ticker: "USDT",
      synth: false,
      trade: false,
      secured: true,
    },
  },
]).then(console.log);
// uploadAndInstantiateExchangeContract();
// executeTxn(EXCHANGE_ADDRESS, {
//   withdraw: {
//     denoms: ["eth-usdt-0xdac17f958d2ee523a2206206994597c13d831ec7"],
//   },
// });
