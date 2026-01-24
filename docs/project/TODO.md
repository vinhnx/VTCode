The Messages endpoint (/v1/messages) will be removed from the xAI API on February 20, 2026.
After that date, any requests sent to /v1/messages will return a 410 Gone error.

We strongly recommend all xAI API users to migrate to our gRPC-based [Chat](https://docs.x.ai/docs/grpc-reference#chat) service or RESTful [Responses API](https://docs.x.ai/docs/api-reference#create-new-response), as these will have access to our latest features.

Check the documentation for using gRPC Chat / Responses API on https://docs.x.ai/docs/guides/chat.