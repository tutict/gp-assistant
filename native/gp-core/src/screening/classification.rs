use crate::market_data::StockItem;
use crate::text::contains_any;

pub(super) fn industry_matches(stock_value: &str, selected_value: &str) -> bool {
    if stock_value == selected_value {
        return true;
    }

    if let Some(aliases) = legacy_industry_aliases(selected_value) {
        return aliases
            .iter()
            .any(|alias| stock_value == *alias || stock_value.contains(alias));
    }

    false
}

fn legacy_industry_aliases(selected_value: &str) -> Option<&'static [&'static str]> {
    match selected_value {
        "银行" => Some(&["银行Ⅱ", "银行服务"]),
        "证券" => Some(&["证券Ⅱ"]),
        "保险" => Some(&["保险Ⅱ"]),
        "中药" => Some(&["中药Ⅱ"]),
        "医药生物" => Some(&[
            "化学制药",
            "中药Ⅱ",
            "生物制品",
            "医疗器械",
            "医疗服务",
            "医药商业",
        ]),
        "食品饮料" => Some(&[
            "白酒Ⅱ",
            "非白酒",
            "食品加工",
            "调味发酵品Ⅱ",
            "饮料乳品",
            "休闲食品",
        ]),
        "白酒" => Some(&["白酒Ⅱ"]),
        "家用电器" => Some(&[
            "白色家电",
            "黑色家电",
            "厨卫电器",
            "小家电",
            "照明设备Ⅱ",
            "家电零部件Ⅱ",
            "其他家电Ⅱ",
        ]),
        "汽车整车" => Some(&["乘用车", "商用车", "摩托车及其他"]),
        "零部件" => Some(&["汽车零部件"]),
        "电力设备" => Some(&[
            "电池",
            "电机Ⅱ",
            "电网设备",
            "光伏设备",
            "风电设备",
            "其他电源设备Ⅱ",
        ]),
        "光伏" => Some(&["光伏设备"]),
        "风电" => Some(&["风电设备"]),
        "有色金属" => Some(&["工业金属", "贵金属", "能源金属", "小金属", "金属新材料"]),
        "钢铁" => Some(&["普钢", "特钢Ⅱ", "冶钢原料"]),
        "煤炭" => Some(&["煤炭开采", "焦炭Ⅱ"]),
        "石油" => Some(&["炼化及贸易", "油服工程", "油气开采Ⅱ"]),
        "化工" => Some(&[
            "化学纤维",
            "化学原料",
            "化学制品",
            "农化制品",
            "塑料",
            "橡胶",
            "非金属材料Ⅱ",
        ]),
        "建材" => Some(&["水泥", "玻璃玻纤", "装修建材"]),
        "建筑装饰" => Some(&[
            "房屋建设Ⅱ",
            "基础建设",
            "专业工程",
            "工程咨询服务Ⅱ",
            "装修装饰Ⅱ",
        ]),
        "计算机" => Some(&["计算机设备", "软件开发", "IT服务Ⅱ"]),
        "软件" => Some(&["软件开发"]),
        "通信" => Some(&["通信服务", "通信设备"]),
        "传媒" => Some(&[
            "出版",
            "电视广播Ⅱ",
            "广告营销",
            "数字媒体",
            "影视院线",
            "游戏Ⅱ",
        ]),
        "国防军工" => Some(&[
            "地面兵装Ⅱ",
            "航海装备Ⅱ",
            "航空装备Ⅱ",
            "航天装备Ⅱ",
            "军工电子Ⅱ",
        ]),
        "航空航天" => Some(&["航空装备Ⅱ", "航天装备Ⅱ"]),
        "机械设备" => Some(&[
            "工程机械",
            "轨交设备Ⅱ",
            "通用设备",
            "专用设备",
            "自动化设备",
        ]),
        "环保" => Some(&["环保设备Ⅱ", "环境治理"]),
        "农业" => Some(&[
            "动物保健Ⅱ",
            "林业Ⅱ",
            "农产品加工",
            "农业综合Ⅱ",
            "饲料",
            "养殖业",
            "渔业",
            "种植业",
        ]),
        "纺织服装" => Some(&["纺织制造", "服装家纺", "饰品"]),
        "商贸零售" => Some(&["贸易Ⅱ", "一般零售", "互联网电商", "专业连锁Ⅱ"]),
        "社会服务" => Some(&[
            "教育",
            "酒店餐饮",
            "旅游及景区",
            "旅游零售Ⅱ",
            "体育Ⅱ",
            "专业服务",
        ]),
        _ => None,
    }
}

pub(super) fn market_scope_for_stock(stock: &StockItem) -> &'static str {
    let code = stock.code.trim().to_ascii_uppercase();
    let digits = code.split('.').next().unwrap_or("");
    if code.ends_with(".BJ")
        || digits.starts_with('8')
        || digits.starts_with('4')
        || digits.starts_with('9')
    {
        "北交所"
    } else if digits.starts_with("688") || digits.starts_with("689") {
        "科创板"
    } else if digits.starts_with("300") || digits.starts_with("301") {
        "创业板"
    } else if code.ends_with(".SH") || digits.starts_with('6') {
        "沪市A股"
    } else {
        "深市A股"
    }
}

const THEME_RULES: [(&str, &str, f64, &[&str]); 7] = [
    (
        "materials",
        "\u{65b0}\u{6750}\u{6599}",
        0.96,
        &[
            "\u{6c1f}\u{5316}\u{5de5}",
            "\u{6c1f}\u{6750}\u{6599}",
            "\u{9502}\u{7535}\u{6750}\u{6599}",
            "\u{7535}\u{89e3}\u{6db2}",
            "\u{516d}\u{6c1f}\u{78f7}\u{9178}\u{9502}",
            "\u{65b0}\u{80fd}\u{6750}",
            "\u{65b0}\u{6750}\u{6599}",
            "\u{56fa}\u{6001}\u{7535}\u{6c60}",
            "\u{78c1}\u{6750}",
        ],
    ),
    (
        "semiconductor_wafer",
        "\u{534a}\u{5bfc}\u{4f53}\u{6676}\u{5706}",
        0.9,
        &[
            "\u{534a}\u{5bfc}\u{4f53}\u{6676}\u{5706}",
            "\u{6676}\u{5706}",
            "\u{6676}\u{5706}\u{4ee3}\u{5de5}",
            "\u{6676}\u{5706}\u{5236}\u{9020}",
            "\u{6676}\u{5706}\u{5382}",
            "\u{7845}\u{6676}\u{5706}",
            "\u{7845}\u{7247}",
            "\u{5916}\u{5ef6}\u{7247}",
            "\u{5916}\u{5ef6}\u{7845}\u{7247}",
            "\u{534a}\u{5bfc}\u{4f53}\u{886c}\u{5e95}",
            "\u{886c}\u{5e95}",
            "\u{78b3}\u{5316}\u{7845}\u{886c}\u{5e95}",
            "sic\u{886c}\u{5e95}",
            "\u{629b}\u{5149}\u{7247}",
            "8\u{82f1}\u{5bf8}",
            "12\u{82f1}\u{5bf8}",
        ],
    ),
    (
        "ai_chain",
        "AI\u{7b97}\u{529b}\u{4e0e}\u{82af}\u{7247}",
        0.95,
        &[
            "\u{534a}\u{5bfc}\u{4f53}",
            "\u{82af}\u{7247}",
            "\u{7b97}\u{529b}",
            "\u{4eba}\u{5de5}\u{667a}\u{80fd}",
            "ai",
            "\u{5149}\u{6a21}\u{5757}",
            "cpo",
            "\u{670d}\u{52a1}\u{5668}",
            "\u{6db2}\u{51b7}",
            "gpu",
            "hbm",
            "\u{5b58}\u{50a8}",
            "\u{6570}\u{636e}\u{4e2d}\u{5fc3}",
            "\u{4e91}\u{8ba1}\u{7b97}",
            "\u{5927}\u{6a21}\u{578b}",
            "aigc",
            "\u{8fb9}\u{7f18}\u{8ba1}\u{7b97}",
            "pcb",
            "\u{5c01}\u{88c5}",
            "\u{5c01}\u{6d4b}",
            "eda",
            "soc",
        ],
    ),
    (
        "tech",
        "\u{79d1}\u{6280}\u{5236}\u{9020}",
        0.84,
        &[
            "\u{673a}\u{5668}\u{4eba}",
            "\u{8f6f}\u{4ef6}",
            "\u{901a}\u{4fe1}",
            "\u{79d1}\u{6280}",
            "\u{7535}\u{5b50}",
            "\u{81ea}\u{52a8}\u{5316}",
            "\u{9ad8}\u{7aef}\u{5236}\u{9020}",
            "\u{667a}\u{80fd}\u{5236}\u{9020}",
        ],
    ),
    (
        "energy",
        "\u{65b0}\u{80fd}\u{6e90}",
        0.82,
        &[
            "\u{65b0}\u{80fd}\u{6e90}",
            "\u{7535}\u{6c60}",
            "\u{50a8}\u{80fd}",
            "\u{5149}\u{4f0f}",
            "\u{7535}\u{529b}",
            "\u{80fd}\u{6e90}",
            "\u{6cb9}\u{6c14}",
            "\u{7164}\u{70ad}",
            "\u{98ce}\u{7535}",
            "\u{5145}\u{7535}\u{6869}",
        ],
    ),
    (
        "medical",
        "\u{533b}\u{836f}\u{533b}\u{7597}",
        0.8,
        &[
            "\u{533b}\u{836f}",
            "\u{533b}\u{7597}",
            "\u{751f}\u{7269}\u{5236}\u{54c1}",
            "\u{521b}\u{65b0}\u{836f}",
            "\u{4e2d}\u{836f}",
            "\u{5316}\u{5b66}\u{5236}\u{836f}",
            "\u{533b}\u{7597}\u{5668}\u{68b0}",
            "cro",
            "cxo",
            "\u{75ab}\u{82d7}",
            "\u{533b}\u{7597}\u{670d}\u{52a1}",
            "\u{4eff}\u{5236}\u{836f}",
        ],
    ),
    (
        "game",
        "\u{6e38}\u{620f}\u{4f20}\u{5a92}",
        0.78,
        &[
            "\u{6e38}\u{620f}",
            "\u{7f51}\u{7edc}\u{6e38}\u{620f}",
            "\u{624b}\u{6e38}",
            "\u{7535}\u{7ade}",
            "\u{4e91}\u{6e38}\u{620f}",
            "\u{4e92}\u{52a8}\u{5a31}\u{4e50}",
            "\u{6587}\u{5316}\u{4f20}\u{5a92}",
            "\u{4f20}\u{5a92}",
        ],
    ),
];

const CONCEPT_GROUP_RULES: [(&str, &[&str]); 21] = [
    (
        "半导体设计",
        &[
            "半导体设计",
            "芯片设计",
            "集成电路设计",
            "ic设计",
            "soc",
            "mcu",
            "模拟芯片",
            "功率芯片",
            "射频芯片",
            "传感器芯片",
            "eda",
        ],
    ),
    (
        "半导体设备",
        &[
            "半导体设备",
            "光刻机",
            "刻蚀",
            "薄膜沉积",
            "离子注入",
            "量测设备",
            "检测设备",
            "清洗设备",
            "涂胶显影",
            "测试设备",
            "探针台",
            "分选机",
        ],
    ),
    (
        "半导体材料",
        &[
            "半导体材料",
            "光刻胶",
            "电子特气",
            "湿电子化学品",
            "靶材",
            "抛光液",
            "抛光垫",
            "cmp",
            "硅片",
            "衬底",
            "碳化硅",
            "sic",
            "氮化镓",
            "gan",
        ],
    ),
    (
        "半导体晶圆",
        &[
            "半导体晶圆",
            "晶圆",
            "晶圆代工",
            "晶圆制造",
            "晶圆厂",
            "硅晶圆",
            "外延片",
            "外延硅片",
            "半导体衬底",
            "碳化硅衬底",
            "sic衬底",
            "抛光片",
            "8英寸",
            "12英寸",
        ],
    ),
    (
        "半导体封测",
        &[
            "封测",
            "封装测试",
            "半导体封装",
            "芯片封装",
            "先进封装",
            "测试服务",
            "晶圆测试",
            "chiplet",
            "2.5d",
            "3d封装",
            "cowos",
        ],
    ),
    (
        "存储芯片",
        &[
            "存储芯片",
            "存储器",
            "dram",
            "nand",
            "nor flash",
            "hbm",
            "固态硬盘",
            "ssd",
            "闪存",
            "内存",
        ],
    ),
    (
        "AI算力与芯片",
        &[
            "算力",
            "人工智能",
            "ai",
            "光模块",
            "cpo",
            "服务器",
            "液冷",
            "gpu",
            "数据中心",
            "云计算",
            "大模型",
            "aigc",
            "边缘计算",
            "pcb",
        ],
    ),
    ("半导体", &["半导体", "集成电路", "芯片制造", "芯片产业"]),
    (
        "新材料",
        &[
            "氟化工",
            "氟材料",
            "锂电材料",
            "电解液",
            "六氟磷酸锂",
            "新能材",
            "新材料",
            "固态电池",
            "磁材",
        ],
    ),
    (
        "新能源与储能",
        &[
            "新能源",
            "电池",
            "储能",
            "光伏",
            "电力",
            "能源",
            "风电",
            "充电桩",
        ],
    ),
    (
        "游戏传媒",
        &[
            "游戏",
            "网络游戏",
            "手游",
            "电竞",
            "云游戏",
            "互动娱乐",
            "传媒",
            "广告营销",
        ],
    ),
    (
        "机器人与高端制造",
        &[
            "机器人",
            "工业母机",
            "自动化",
            "高端制造",
            "智能制造",
            "机械设备",
        ],
    ),
    (
        "消费零售",
        &[
            "食品",
            "饮料",
            "白酒",
            "休闲食品",
            "一般零售",
            "商贸零售",
            "家电",
            "旅游",
            "酒店",
            "餐饮",
        ],
    ),
    (
        "医药医疗",
        &[
            "医药",
            "医疗",
            "生物制品",
            "创新药",
            "中药",
            "化学制药",
            "医疗器械",
            "cro",
        ],
    ),
    (
        "金融地产",
        &["银行", "证券", "保险", "房地产", "地产", "物业"],
    ),
    (
        "基建建筑",
        &[
            "建筑",
            "房屋建设",
            "工程建设",
            "基础建设",
            "水泥",
            "铁路",
            "公路",
            "装修装饰",
        ],
    ),
    (
        "周期资源",
        &[
            "煤炭", "钢铁", "普钢", "有色", "金属", "化工", "石油", "油气", "矿业",
        ],
    ),
    (
        "汽车产业链",
        &[
            "汽车",
            "整车",
            "零部件",
            "轮胎",
            "智能驾驶",
            "无人驾驶",
            "汽车服务",
        ],
    ),
    (
        "军工航天",
        &["军工", "航天", "航空", "卫星", "船舶", "无人机", "国防"],
    ),
    (
        "交运物流",
        &[
            "物流",
            "航运",
            "港口",
            "机场",
            "航空运输",
            "铁路运输",
            "快递",
        ],
    ),
    ("公用环保", &["环保", "水务", "燃气", "供热", "公用事业"]),
];

const COLD_SECTOR_KEYWORDS: [&str; 9] = [
    "\u{94f6}\u{884c}",
    "\u{57fa}\u{5efa}",
    "\u{5efa}\u{7b51}",
    "\u{5efa}\u{7b51}\u{88c5}\u{9970}",
    "\u{5de5}\u{7a0b}\u{5efa}\u{8bbe}",
    "\u{57fa}\u{7840}\u{5efa}\u{8bbe}",
    "\u{6c34}\u{6ce5}",
    "\u{94c1}\u{8def}",
    "\u{516c}\u{8def}",
];

pub(crate) fn concept_group_for_stock(stock: &StockItem) -> String {
    let text = stock_text(stock);
    for (label, keywords) in CONCEPT_GROUP_RULES {
        if contains_any(&text, keywords) {
            return label.to_string();
        }
    }
    "\u{5176}\u{4ed6}\u{6982}\u{5ff5}".to_string()
}

pub(super) fn concept_sector_group_for_stock(stock: &StockItem) -> String {
    let concept = concept_group_for_stock(stock);
    let semiconductor_group = matches!(
        concept.as_str(),
        "半导体设计"
            | "半导体设备"
            | "半导体材料"
            | "半导体晶圆"
            | "半导体封测"
            | "存储芯片"
            | "半导体"
    ) || (concept == "AI算力与芯片"
        && contains_any(
            &stock_text(stock),
            &["半导体", "集成电路", "芯片制造", "芯片产业"],
        ));
    if semiconductor_group {
        "半导体".to_string()
    } else {
        concept
    }
}

pub(super) fn concept_rank(concept: &str) -> usize {
    CONCEPT_GROUP_RULES
        .iter()
        .position(|(label, _)| *label == concept)
        .unwrap_or(CONCEPT_GROUP_RULES.len())
}

pub(super) fn board_group_for_stock(stock: &StockItem) -> &'static str {
    let code = stock.code.trim().to_ascii_uppercase();
    let digits = code.split('.').next().unwrap_or("");
    if code.ends_with(".BJ")
        || digits.starts_with('8')
        || digits.starts_with('4')
        || digits.starts_with('9')
    {
        "\u{5317}\u{4ea4}\u{6240}"
    } else if digits.starts_with("688") {
        "\u{79d1}\u{521b}\u{677f}"
    } else if digits.starts_with("300") || digits.starts_with("301") {
        "\u{521b}\u{4e1a}\u{677f}"
    } else if code.ends_with(".SH") || digits.starts_with('6') {
        "\u{6caa}\u{4e3b}\u{677f}"
    } else {
        "\u{6df1}\u{4e3b}\u{677f}"
    }
}

pub(super) fn board_rank(board: &str) -> usize {
    match board {
        "\u{79d1}\u{521b}\u{677f}" => 0,
        "\u{521b}\u{4e1a}\u{677f}" => 1,
        "\u{5317}\u{4ea4}\u{6240}" => 2,
        "\u{6caa}\u{4e3b}\u{677f}" => 3,
        "\u{6df1}\u{4e3b}\u{677f}" => 4,
        _ => 99,
    }
}

pub(crate) fn theme_match_for_stock(
    stock: &StockItem,
) -> Option<(&'static str, &'static str, f64)> {
    theme_match_for_text(&stock_text(stock))
}

fn theme_match_for_text(text: &str) -> Option<(&'static str, &'static str, f64)> {
    for (key, label, score, keywords) in THEME_RULES {
        if contains_any(text, keywords) {
            return Some((key, label, score));
        }
    }
    None
}

fn stock_text(stock: &StockItem) -> String {
    if stock.code.ends_with(".HK") && !stock.industry.trim().is_empty() {
        return stock.industry.to_lowercase();
    }
    format!("{} {}", stock.name, stock.industry).to_lowercase()
}

pub(super) fn is_cold_sector(industry: &str) -> bool {
    let normalized = industry.trim().to_lowercase();
    if normalized.is_empty() {
        return false;
    }
    COLD_SECTOR_KEYWORDS
        .iter()
        .any(|keyword| normalized.contains(keyword))
}
