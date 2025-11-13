
[1m[96mNetGet - LLM-Controlled Network Protocols[0m

[2mNetGet is an experimental network application where an LLM (via Ollama)[0m
[2mcontrols network protocols and acts as a server for 50+ protocols.[0m
[2mAll protocol logic is handled by the LLM - you describe behavior in natural language.[0m

[1mKey Features:[0m
  [32m•[0m [1mScripting:[0m LLM generates on-the-fly Python/JavaScript code to reduce LLM calls
  [32m•[0m [1mWeb Search:[0m LLM can fetch protocol RFCs and documentation from the web
  [32m•[0m [1mFile Reading:[0m LLM can read local files (schemas, configs, prompts)
  [32m•[0m [1mLogging:[0m Comprehensive logging system (TRACE/DEBUG/INFO/WARN/ERROR levels)
  [32m•[0m [1mAction-Based:[0m Structured JSON responses for precise protocol control
  [32m•[0m [1mDynamic Reconfiguration:[0m Change server behavior at runtime without restart

[1mAvailable Protocols:[0m
[2mUse[0m [36m[1m/docs <protocol>[0m [2mto see detailed information.[0m

[92m━━━ AI & API ━━━[0m
  [2m[33mJSON-RPC[0m[0m, [2m[33mMCP[0m[0m, [2m[33mOAuth2[0m[0m, [2m[33mOllama[0m[0m, [2m[34mOpenAI[0m[0m, [2m[33mOpenAPI[0m[0m, [2m[33mXML-RPC[0m[0m, [2m[33mgRPC[0m

[92m━━━ Application ━━━[0m
  [2m[33mAMQP[0m[0m, [2m[33mDC[0m[0m, [2m[33mIMAP[0m[0m, [2m[33mIRC[0m[0m, [2m[33mLDAP[0m[0m, [2m[33mMQTT[0m[0m, [2m[33mMaven[0m[0m, [2m[33mNNTP[0m[0m, [2m[33mPOP3[0m[0m, [2m[33mPyPI[0m[0m, [2m[33mSMTP[0m[0m, [2m[33mTelnet[0m[0m, [2m[33mXMPP[0m[0m, [2m[33mmDNS[0m

[92m━━━ Authentication ━━━[0m
  [2m[33mOpenID[0m[0m, [2m[33mSamlIdp[0m[0m, [2m[33mSamlSp[0m

[92m━━━ Blockchain ━━━[0m
  [2m[33mBitcoin P2P[0m

[92m━━━ Core ━━━[0m
  [2m[33mARP[0m[0m, [2m[33mBOOTP[0m[0m, [2m[34mDHCP[0m[0m, [2m[34mDNS[0m[0m, [2m[34mDataLink[0m[0m, [2m[34mDoH[0m[0m, [2m[34mDoT[0m[0m, [2m[34mHTTP[0m[0m, [2m[33mHTTP2[0m[0m, [2m[33mHTTP3[0m[0m, [2m[34mNTP[0m[0m, [2m[34mSNMP[0m[0m, [2m[33mSOCKET_FILE[0m[0m, [2m[34mSSH[0m[0m, [2m[33mSyslog[0m[0m, [2m[34mTCP[0m[0m, [2m[33mTLS[0m[0m, [2m[34mUDP[0m[0m, [2m[34mWHOIS[0m

[92m━━━ Database ━━━[0m
  [2m[33mCassandra[0m[0m, [2m[33mDynamoDB[0m[0m, [2m[33mElasticsearch[0m[0m, [2m[33mKAFKA[0m[0m, [2m[33mMySQL[0m[0m, [2m[33mPostgreSQL[0m[0m, [2m[33mRedis[0m[0m, [2m[33mSQS[0m[0m, [2m[33mZooKeeper[0m[0m, [2m[33metcd[0m

[92m━━━ Experimental ━━━[0m
  [2m[33mISIS[0m

[92m━━━ Infrastructure ━━━[0m
  [2m[33mSVN[0m

[92m━━━ NFC & Smart Cards ━━━[0m
  [2m[31mnfc[0m

[92m━━━ Network ━━━[0m
  [2m[33mBLUETOOTH_BLE[0m[0m, [2m[33mBLUETOOTH_BLE_BATTERY[0m[0m, [2m[33mBLUETOOTH_BLE_BEACON[0m[0m, [2m[33mBLUETOOTH_BLE_CYCLING[0m[0m, [2m[33mBLUETOOTH_BLE_DATA_STREAM[0m[0m, [2m[33mBLUETOOTH_BLE_ENVIRONMENTAL[0m[0m, [2m[33mBLUETOOTH_BLE_FILE_TRANSFER[0m[0m, [2m[33mBLUETOOTH_BLE_GAMEPAD[0m[0m, [2m[33mBLUETOOTH_BLE_HEART_RATE[0m[0m, [2m[33mBLUETOOTH_BLE_KEYBOARD[0m[0m, [2m[33mBLUETOOTH_BLE_MOUSE[0m[0m, [2m[33mBLUETOOTH_BLE_PRESENTER[0m[0m, [2m[33mBLUETOOTH_BLE_PROXIMITY[0m[0m, [2m[33mBLUETOOTH_BLE_REMOTE[0m[0m, [2m[33mBLUETOOTH_BLE_RUNNING[0m[0m, [2m[33mBLUETOOTH_BLE_THERMOMETER[0m[0m, [2m[33mBLUETOOTH_BLE_WEIGHT_SCALE[0m[0m, [2m[33mIGMP[0m[0m, [2m[33mRIP[0m

[92m━━━ Network Services ━━━[0m
  [2m[33mTor Directory[0m[0m, [2m[32mTor Relay[0m[0m, [2m[33mVNC[0m

[92m━━━ P2P ━━━[0m
  [2m[33mTorrent-DHT[0m[0m, [2m[33mTorrent-Peer[0m[0m, [2m[33mTorrent-Tracker[0m

[92m━━━ Package Management ━━━[0m
  [2m[33mNPM[0m

[92m━━━ Proxy & Network ━━━[0m
  [2m[33mProxy[0m[0m, [2m[33mSIP[0m[0m, [2m[33mSOCKS5[0m[0m, [2m[33mSTUN[0m[0m, [2m[33mTURN[0m

[92m━━━ Security ━━━[0m
  [2m[33mSSH Agent[0m

[92m━━━ USB ━━━[0m
  [2m[33musb-fido2[0m

[92m━━━ USB Devices ━━━[0m
  [2m[33mUSB-Keyboard[0m[0m, [2m[33mUSB-MassStorage[0m[0m, [2m[33mUSB-Mouse[0m[0m, [2m[33mUSB-Serial[0m[0m, [2m[31musb-smartcard[0m

[92m━━━ VPN & Routing ━━━[0m
  [2m[31mBGP[0m[0m, [2m[33mIPSec/IKEv2[0m[0m, [2m[33mOSPF[0m[0m, [2m[32mOpenVPN[0m[0m, [2m[32mWireGuard[0m

[92m━━━ Web ━━━[0m
  [2m[33mRSS[0m

[92m━━━ Web & File ━━━[0m
  [2m[33mGit[0m[0m, [2m[33mIPP[0m[0m, [2m[33mMercurial[0m[0m, [2m[33mNFS[0m[0m, [2m[33mS3[0m[0m, [2m[33mSMB[0m[0m, [2m[33mWebDAV[0m

