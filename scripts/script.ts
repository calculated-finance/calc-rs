import { CodeDetails, SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { stringToPath } from "@cosmjs/crypto";
import { DirectSecp256k1HdWallet, type Coin } from "@cosmjs/proto-signing";
import { GasPrice, StargateClient } from "@cosmjs/stargate";
import { config } from "dotenv";
import fs from "fs";

const MANAGER_ADDRESS =
  "sthor1xg6qsvyktr0zyyck3d67mgae0zun4lhwwn3v9pqkl5pk8mvkxsnscenkc0";

const EXCHANGE_ADDRESS =
  "sthor196c0zhmpaktqu3hfgdafvsdlr3x9tz0n78qvwn7g7g2c7zmaa0jqxcd6st";

const SCHEDULER_ADDRESS =
  "sthor1dvdcm5r08utc9axjhywuw3e8lq2q4tfnmxgjg7mtf2s8mtl959fqg8nr8v";

const getWallet = async () =>
  DirectSecp256k1HdWallet.fromMnemonic(process.env.MNEMONIC!, {
    prefix: process.env.PREFIX! || "sthor",
    hdPaths: [stringToPath(`m/44'/931'/0'/0/0`)],
  });

const getSigner = async () =>
  SigningCosmWasmClient.connectWithSigner(
    process.env.RPC_URL!,
    await getWallet(),
    {
      gasPrice: GasPrice.fromString(process.env.GAS_PRICE || "0.0urune"),
    }
  );

export const upload = async (binaryFilePath: string) => {
  const wallet = await getWallet();
  const cosmWasmClient = await getSigner();
  const adminAddress = await getAccount(wallet);

  const { codeId } = await cosmWasmClient.upload(
    adminAddress,
    fs.readFileSync(binaryFilePath),
    1.5
  );

  return codeId;
};

export const uploadAndInstantiate = async (
  binaryFilePath: string,
  adminAddress: string,
  initMsg: Record<string, unknown>,
  label: string,
  funds: Coin[] = []
): Promise<string> => {
  const cosmWasmClient = await getSigner();

  const { codeId } = await cosmWasmClient.upload(
    adminAddress,
    fs.readFileSync(binaryFilePath),
    1.5
  );

  console.log("Uploaded code id:", codeId);

  const { contractAddress } = await cosmWasmClient.instantiate(
    adminAddress,
    codeId,
    initMsg,
    label,
    1.5,
    { funds, admin: adminAddress }
  );

  console.log(label, "contract address:", contractAddress);

  return contractAddress;
};

export const uploadAndMigrate = async (
  binaryFilePath: string,
  adminAddress: string,
  contractAddress: string,
  msg: Record<string, unknown> = {}
): Promise<void> => {
  const cosmWasmClient = await getSigner();
  const { codeId } = await cosmWasmClient.upload(
    adminAddress,
    fs.readFileSync(binaryFilePath),
    1.5
  );

  console.log("Uploaded code id:", codeId);

  await cosmWasmClient.migrate(
    adminAddress,
    contractAddress,
    codeId,
    msg,
    "auto"
  );

  console.log("Migrated contract at address:", contractAddress);
};

export const getAccount = async (wallet: DirectSecp256k1HdWallet) => {
  const accounts = await wallet.getAccounts();
  return accounts[0]?.address;
};

config();

const uploadStrategyContract = async () => {
  const codeId = await upload("artifacts/strategy.wasm");

  console.log("Strategy contract code ID:", codeId);

  return codeId;
};

const uploadAndInstantiateManagerContract = async (config: {
  code_id: number;
  checksum: string;
}) => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndInstantiate(
    "artifacts/manager.wasm",
    adminAddress,
    {
      ...config,
      fee_collector: adminAddress,
    },
    "Manager Contract"
  );
};

const uploadAndInstantiateExchangeContract = async () => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndInstantiate(
    "artifacts/exchange.wasm",
    adminAddress,
    {},
    "Exchange Contract"
  );
};

const uploadAndMigrateManagerContract = async (config?: {
  code_id: number;
  checksum: string;
}) => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndMigrate(
    "artifacts/manager.wasm",
    adminAddress,
    MANAGER_ADDRESS,
    {
      ...(config || (await getConfig(MANAGER_ADDRESS))),
      fee_collector: adminAddress,
    }
  );
};

const uploadAndMigrateExchangeContract = async () => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndMigrate(
    "artifacts/exchange.wasm",
    adminAddress,
    EXCHANGE_ADDRESS
  );
};

const uploadAndMigrateSchedulerContract = async () => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndMigrate(
    "artifacts/scheduler.wasm",
    adminAddress,
    SCHEDULER_ADDRESS
  );
};

const uploadAndInstantiateSchedulerContract = async () => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndInstantiate(
    "artifacts/scheduler.wasm",
    adminAddress,
    {},
    "Scheduler Contract"
  );
};

const getCodeDetails = async (codeId: number): Promise<CodeDetails> => {
  const cosmWasmClient = await getSigner();
  const info = await cosmWasmClient.getCodeDetails(codeId);

  console.log("Code details:", info);

  return info;
};

const uploadAndInstantiateContractSuite = async () => {
  const strategyCodeId = await uploadStrategyContract();
  const codeDetails = await getCodeDetails(strategyCodeId);
  await uploadAndInstantiateManagerContract({
    code_id: strategyCodeId,
    checksum: codeDetails.checksum,
  });
  await uploadAndInstantiateExchangeContract();
  await uploadAndInstantiateSchedulerContract();
};

const uploadAndMigrateContractSuite = async () => {
  const strategyCodeId = await uploadStrategyContract();
  const codeDetails = await getCodeDetails(strategyCodeId);
  await uploadAndMigrateManagerContract({
    code_id: strategyCodeId,
    checksum: codeDetails.checksum,
  });
  await uploadAndMigrateExchangeContract();
  await uploadAndMigrateSchedulerContract();
};

const uploadPairs = async () => {
  const cosmWasmClient = await getSigner();

  const account = await getAccount(await getWallet());

  await cosmWasmClient.execute(
    account,
    SCHEDULER_ADDRESS,
    {
      create_pairs: {
        pairs: [{}],
      },
    },
    "auto"
  );
};

const fetchBalances = async (address: string) => {
  const stargateClient = await StargateClient.connect(process.env.RPC_URL!);
  const balances = await stargateClient.getAllBalances(address);

  console.log("Balances:", balances);

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

  console.log("Can swap response:", response);
};

const getExpectedReceiveAmount = async () => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(EXCHANGE_ADDRESS, {
    expected_receive_amount: {
      swap_amount: {
        denom: "eth-usdt-0xdac17f958d2ee523a2206206994597c13d831ec7",
        amount: "10000000",
      },
      target_denom: "rune",
    },
  });

  console.log("Expected receive amount:", response);
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

  console.log("Spot price:", response);
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

  console.log("Route:", response);
};

const swap = async () => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWallet());
  const response = await cosmWasmClient.execute(
    account,
    EXCHANGE_ADDRESS,
    {
      swap: {
        minimum_receive_amount: {
          denom: "rune",
          amount: "1",
        },
      },
    },
    "auto",
    "Swap",
    [
      {
        denom: "eth-usdt-0xdac17f958d2ee523a2206206994597c13d831ec7",
        amount: "10000000",
      },
    ]
  );
  console.log("Swap response:", response);
};

const getConfig = async (contractAddress: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(contractAddress, {
    config: {},
  });

  console.log("Config:", response);

  return response;
};

const createStrategy = async () => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWallet());
  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      instantiate_strategy: {
        owner: account,
        label: "Test Strategy",
        strategy: {
          twap: {
            owner: account,
            swap_amount: {
              denom: "eth-usdt-0xdac17f958d2ee523a2206206994597c13d831ec7",
              amount: "10000000",
            },
            minimum_receive_amount: {
              denom: "rune",
              amount: "1",
            },
            schedule: {
              time: {
                duration: {
                  nanos: 0,
                  secs: 1,
                },
              },
            },
            exchange_contract: EXCHANGE_ADDRESS,
            scheduler_contract: SCHEDULER_ADDRESS,
            execution_rebate: {
              denom: "rune",
              amount: "0",
            },
            mutable_destinations: [
              {
                address: account,
                shares: "10000",
                label: "Me",
              },
            ],
            immutable_destinations: [],
          },
        },
      },
    },
    "auto",
    "Create Strategy",
    [
      {
        denom: "eth-usdt-0xdac17f958d2ee523a2206206994597c13d831ec7",
        amount: "100124758",
      },
    ]
  );

  console.log("Create strategy response:", response);

  const strategies = await cosmWasmClient.queryContractSmart(MANAGER_ADDRESS, {
    strategies: {
      address: account,
    },
  });

  console.log("Strategies:", strategies);
};

const getStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(MANAGER_ADDRESS, {
    strategy: {
      address,
    },
  });
  console.log("Strategy:", response);
};

const getStrategyConfig = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(address, {
    config: {},
  });

  console.log("Strategy Config:", response);
};

const getStrategies = async () => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(MANAGER_ADDRESS, {
    strategies: {},
  });
  console.log("Strategies:", response);
};

const executeStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWallet());
  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      execute_strategy: {
        contract_address: address,
      },
    },
    "auto"
  );

  console.log("Execute Strategy Response:", response);
};

const executeTriggers = async (owner: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWallet());

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
      "auto"
    );

    console.log("Execute Trigger Response:", response);
  }
};

const resumeStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWallet());
  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      resume_strategy: {
        contract_address: address,
      },
    },
    "auto"
  );

  console.log("Resume Strategy Response:", response);
};

const withdrawFromStrategy = async (address: string) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWallet());
  const balances = await fetchBalances(address);
  const response = await cosmWasmClient.execute(
    account,
    MANAGER_ADDRESS,
    {
      withdraw_from_strategy: {
        contract_address: address,
        amounts: balances,
      },
    },
    "auto"
  );

  console.log("Withdraw Response:", response);
};

const customQuery = async (
  contractAddress: string,
  msg: Record<string, unknown>
) => {
  const cosmWasmClient = await getSigner();
  const response = await cosmWasmClient.queryContractSmart(
    contractAddress,
    msg
  );
  console.log("Custom Query Response:", response);
  return response;
};

const customExecute = async (
  contractAddress: string,
  msg: Record<string, unknown>,
  funds: Coin[] = []
) => {
  const cosmWasmClient = await getSigner();
  const account = await getAccount(await getWallet());
  const response = await cosmWasmClient.execute(
    account,
    contractAddress,
    msg,
    "auto",
    undefined,
    funds
  );

  console.log("Custom Execute Response:", response);
};

const getFinBook = async (pairAddress: string) => {
  const cosmWasmClient = await getSigner();

  const book = await cosmWasmClient.queryContractSmart(pairAddress, {
    book: {
      limit: 1,
    },
  });

  console.log("Financial Book:", book);
};

const getMyBalances = async () => {
  await fetchBalances(await getAccount(await getWallet()));
};

const fetchFinBook = async (pairAddress: string) => {
  const cosmWasmClient = await getSigner();
  const book = await cosmWasmClient.queryContractSmart(pairAddress, {
    book: {
      limit: 10,
    },
  });

  console.log("Financial Book:", book);
};

const STRATEGY_ADDRESS =
  "sthor10tnuxn9u5ylsnn8qnqe2gtu4xhh5lxcwgfmxqw3gj2nhy2c7mylslnv3ue";

const PAIR_ADDRESS =
  "sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r";

// uploadContractSuite();
// fetchBalances(STRATEGY_ADDRESS);
// getMyBalances();
// fetchFinBook(PAIR_ADDRESS);
// createStrategy();
// getStrategy(STRATEGY_ADDRESS);
// getStrategies();
// getConfig(PAIR_ADDRESS);
// executeStrategy(STRATEGY_ADDRESS);
// executeTriggers(STRATEGY_ADDRESS).then(() =>
//   getStrategyConfig(STRATEGY_ADDRESS)
// );
// withdrawFromStrategy(STRATEGY_ADDRESS);
// uploadAndMigrateExchangeContract();
// uploadAndMigrateSchedulerContract();
// uploadAndMigrateManagerContract();
// resumeStrategy(STRATEGY_ADDRESS);
// uploadAndMigrateContractSuite();
// uploadContractSuite();
// getFinBook("sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r");
// canSwap();
// getSpotPrice();
// getExpectedReceiveAmount();
// getRoute();
// swap();
// uploadAndInstantiateExchangeContract();
// customExecute(EXCHANGE_ADDRESS, {
//   custom: Buffer.from(
//     JSON.stringify({
//       create_pairs: {
//         pairs: [
//           {
//             quote_denom: "rune",
//             base_denom: "x/ruji",
//             address:
//               "sthor1knzcsjqu3wpgm0ausx6w0th48kvl2wvtqzmvud4hgst4ggutehlseele4r",
//           },
//         ],
//       },
//     })
//   ).toBase64(),
// });
