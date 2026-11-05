SELECT EXISTS(SELECT 1 FROM pragma_table_info('threads') WHERE name = 'name')
