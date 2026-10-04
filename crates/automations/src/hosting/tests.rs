use super::*;

#[test]
fn finish_marker_must_be_the_final_standalone_line_outside_code() {
    for output in [
        "[AOW_HOSTING_DONE]",
        "审查通过。\n[AOW_HOSTING_DONE]\n\n",
        "检查通过\r\n[AOW_HOSTING_DONE]  \r\n",
        "```text\n[AOW_HOSTING_DONE]\n```\n[AOW_HOSTING_DONE]",
    ] {
        assert!(review_passed(output), "{output:?}");
    }
    for output in [
        "",
        " \n",
        "审查通过",
        "尚未通过 [AOW_HOSTING_DONE]",
        "[AOW_HOSTING_DONE]\n仍有问题",
        "[aow_hosting_done]",
        "`[AOW_HOSTING_DONE]`",
        "    [AOW_HOSTING_DONE]",
        "\t[AOW_HOSTING_DONE]",
        "> [AOW_HOSTING_DONE]",
        "```text\n[AOW_HOSTING_DONE]\n```",
        "```\n[AOW_HOSTING_DONE]",
        "~~~\n[AOW_HOSTING_DONE]",
        "````\n```\n[AOW_HOSTING_DONE]",
    ] {
        assert!(!review_passed(output), "{output:?}");
    }
}
