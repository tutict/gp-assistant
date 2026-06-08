#ifndef GP_CORE_H
#define GP_CORE_H

#ifdef __cplusplus
extern "C" {
#endif

char *gp_core_screen_json(const char *criteria_json);
char *gp_core_screen_with_data_json(const char *request_json);
char *gp_core_graph_screen_json(const char *request_json);
char *gp_core_graph_screen_with_data_json(const char *request_json);
char *gp_core_backtest_json(const char *request_json);
char *gp_core_backtest_with_data_json(const char *request_json);
char *gp_core_trend_json(const char *request_json);
char *gp_core_trend_with_data_json(const char *request_json);
char *gp_core_trend_screen_json(const char *request_json);
char *gp_core_trend_screen_with_data_json(const char *request_json);
char *gp_core_agent_json(const char *request_json);
char *gp_core_agent_with_data_json(const char *request_json);
char *gp_core_mobile_stock_skill_json(const char *request_json);
char *gp_core_validate_data_source_json(const char *data_json);
void gp_core_free_string(char *ptr);

#ifdef __cplusplus
}
#endif

#endif
