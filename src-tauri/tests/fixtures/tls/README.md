The localhost certificate and private key are disposable test fixtures generated
solely for loopback tests. They are not used by the application or any service.

The public eMudhra leaf, issuer and cross-signed root certificates come from
https://emudhra.we-have-a-problem.com/chains.zip, entry
`chains/dv-g3b-emudhra.we-have-a-problem.com.pem`, retrieved 2026-10-23.
`emudhra-root.der` is the public emSign ECC Root CA - G3 from Node 26.11.1's
historical built-in store, SHA256
`86A1ECBA089C4A8D3BBE2734C612BA341D813E043CF9E8A862CD5C57A36BBE6B`.
The test explicitly trusts the supplied root to demonstrate that the compromised
intermediate is rejected even when a platform/custom trust store trusts its issuer.
No compromised private key is included. The test server uses the unrelated
localhost fixture key; certificate distrust must occur before signature validation.
