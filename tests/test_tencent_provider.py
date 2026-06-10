import unittest

from app.providers.tencent import TencentQuoteClient
from app.providers.tdx import TdxProvider


class TencentQuoteClientTests(unittest.TestCase):
    def test_tdx_provider_keeps_tencent_quotes_direct_when_system_proxy_selected(self):
        provider = TdxProvider(proxy_mode="system")

        self.assertFalse(provider._session.trust_env)

    def test_symbol_normalizes_a_share_markets(self):
        self.assertEqual(TencentQuoteClient.tencent_symbol("600000.SH"), "sh600000")
        self.assertEqual(TencentQuoteClient.tencent_symbol("300750.SZ"), "sz300750")
        self.assertEqual(TencentQuoteClient.tencent_symbol("430001.BJ"), "bj430001")
        self.assertEqual(TencentQuoteClient.tencent_symbol("bj830799"), "bj830799")

    def test_symbol_rejects_invalid_codes(self):
        self.assertEqual(TencentQuoteClient.tencent_symbol(""), "")
        self.assertEqual(TencentQuoteClient.tencent_symbol("abc"), "")
        self.assertEqual(TencentQuoteClient.tencent_symbol("12345"), "")
        self.assertEqual(TencentQuoteClient.tencent_symbol("SHABCDEF"), "")

    def test_batch_size_falls_back_when_invalid(self):
        client = TencentQuoteClient(object(), 1, "bad")

        self.assertEqual(client.batch_size, 80)

    def test_parse_response_extracts_quote_fields(self):
        values = [""] * 53
        values[1] = "浦发银行"
        values[3] = "12.34"
        values[4] = "12.00"
        values[5] = "12.10"
        values[9] = "12.33"
        values[10] = "100"
        values[19] = "12.35"
        values[20] = "120"
        values[30] = "20260610145959"
        values[31] = "0.34"
        values[32] = "2.83"
        values[33] = "12.60"
        values[34] = "11.98"
        values[37] = "12345.6"
        values[38] = "1.23"
        values[39] = "8.5"
        values[43] = "5.1"
        values[44] = "3000"
        values[45] = "2500"
        values[46] = "0.75"
        values[47] = "13.20"
        values[48] = "10.80"
        values[49] = "1.8"
        values[52] = "9.2"
        text = f'v_sh600000="{ "~".join(values) }";'

        quotes = TencentQuoteClient.parse_response(text)

        quote = quotes["600000"]
        self.assertEqual(quote["code"], "600000")
        self.assertEqual(quote["name"], "浦发银行")
        self.assertEqual(quote["price"], 12.34)
        self.assertEqual(quote["last_close"], 12.0)
        self.assertEqual(quote["bid1"], 12.33)
        self.assertEqual(quote["ask1_volume"], 120.0)
        self.assertEqual(quote["timestamp"], "2026-06-10T14:59:59")
        self.assertEqual(quote["pe_ttm"], 8.5)
        self.assertEqual(quote["mcap_yi"], 3000.0)
        self.assertEqual(quote["pb"], 0.75)
        self.assertEqual(quote["pe_static"], 9.2)

    def test_parse_response_skips_malformed_lines(self):
        self.assertEqual(TencentQuoteClient.parse_response('v_sh600000="too~short";'), {})
        self.assertEqual(TencentQuoteClient.parse_response(f'v_unknown="{ "~".join([""] * 53) }";'), {})
        self.assertEqual(TencentQuoteClient.parse_response(""), {})


if __name__ == "__main__":
    unittest.main()
