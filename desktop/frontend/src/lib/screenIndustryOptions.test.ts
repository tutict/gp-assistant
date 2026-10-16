import { describe, expect, it } from "vitest";
import { industryFilterHint } from "./screenIndustryOptions";

describe("industry filter scenarios", () => {
  it("explains that broad industries stay exact while small groups use a sample gate", () => {
    expect(industryFilterHint("银行")).toContain("大类");
    expect(industryFilterHint("银行")).toContain("5");
    expect(industryFilterHint("银行")).toContain("不合并");
  });

  it("keeps specific industries exact", () => {
    expect(industryFilterHint("银行Ⅱ")).toContain("细分行业");
    expect(industryFilterHint("银行Ⅱ")).toContain("严格");
  });

  it("explains the default group sample gate when no industry is selected", () => {
    expect(industryFilterHint("")).toContain("5");
  });
});
