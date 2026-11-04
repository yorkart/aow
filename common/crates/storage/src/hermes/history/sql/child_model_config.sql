CASE WHEN json_valid(child.model_config) THEN child.model_config ELSE '{}' END
