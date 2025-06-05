import { SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";
import { stringToPath } from "@cosmjs/crypto";
import { DirectSecp256k1HdWallet, type Coin } from "@cosmjs/proto-signing";
import { GasPrice, StargateClient } from "@cosmjs/stargate";
import { config } from "dotenv";
import fs from "fs";

const MANAGER_ADDRESS =
  "sthor1xp856u6s4mxxq3zpm8lr2dy6w8z9ttzu26shvheqfw5p75wm06tqxq95a3";

const EXCHANGE_ADDRESS =
  "sthor1qxdh2778l06s6f0t44503x3rqeaz9nma7rnpndmtethw4kp68t5s2vjuuf";

const SCHEDULER_ADDRESS =
  "sthor1ad9wj03d2upe5h68ypzjjqxj7mcqdadysqhqsw5hx9kvr5zs5mlsv3yyzy";

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

const uploadAndInstantiateManagerContract = async (codeId: number) => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndInstantiate(
    "artifacts/manager.wasm",
    adminAddress,
    {
      code_id: codeId,
      checksum:
        "bb8bf7d32a57b6616da5342f5641edf9ff9e667ff0627b9b2b4e8a2de04afbab",
    },
    "Manager Contract"
  );
};

const uploadAndInstantiateExchangeContract = async () => {
  const wallet = await getWallet();
  const adminAddress = await getAccount(wallet);

  await uploadAndInstantiate(
    "artifacts/exchange_fin.wasm",
    adminAddress,
    {},
    "Exchange Contract"
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

// const uploadContractSuite = async () => {
//   const strategyCodeId = await uploadStrategyContract();
//   await uploadAndInstantiateManagerContract(strategyCodeId);
//   await uploadAndInstantiateExchangeContract();
//   await uploadAndInstantiateSchedulerContract();
// };

// uploadContractSuite();

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

const fetchBalances = async () => {
  const stargateClient = await StargateClient.connect(process.env.RPC_URL!);
  const account = await getAccount(await getWallet());

  const balances = await stargateClient.getAllBalances(
    "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka"
  );

  console.log("Balances:", balances);
};

fetchBalances();
