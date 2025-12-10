~/Infra$ trustee run 'install https://documentdb.io/ I think I already downloaded the docker image, then check at what postgresql version it is working and can I use both postgres and documentdb at the same time ?'
Initializing agent in confirm mode (run mode: global)...
Starting new session: install https://documentdb.io/ I think I already downloaded the docker image, then check at what postgresql version it is working and can I use both postgres and documentdb at the same time ?
🔧 Configuration: 📞 NON-STREAMING MODE | Endpoint: unknown | Provider: tanbal | Model: openai/gpt-5-mini
INFO: Session started in confirm mode
INFO: Checkpoint session initialized successfully
Starting workflow execution...
🚀 Using streaming workflow
🚀 Starting TRUE unified streaming workflow
🔥 API Call 1 | Context=689 | Streaming | Model: openai/gpt-5-mini | Tools: 25
🚀 Using provider streaming with umf::StreamingAccumulator
Error: Task failed: Streaming workflow failed
Error: ExecutionError("Agent execution failed: Streaming workflow failed")