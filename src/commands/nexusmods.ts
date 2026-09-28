import { commands, events } from "./bindings";

/** Wrapper around exported commands from Rust. */
const NexusMods = {
  api: {
    validate: commands.nexusmodsApiValidate,
    listModFiles: commands.nexusmodsApiListModFiles,
    requestDownloadLinks: commands.nexusmodsApiRequestDownloadLinks,
  },
  getAccountInfo: commands.nexusmodsGetAccountInfo,
  setAccountInfo: commands.nexusmodsSetAccountInfo,
  deleteAccountInfo: commands.nexusmodsDeleteAccountInfo,
  loginViaSso: commands.nexusmodsLoginViaSso,
  extractDetailsFromNxmUrl: commands.nexusmodsExtractDetailsFromNxmUrl,
  events: {
    ssoAbort: events.ssoAbort,
    ssoUpdate: events.ssoUpdate,
  },
};

export default NexusMods;
