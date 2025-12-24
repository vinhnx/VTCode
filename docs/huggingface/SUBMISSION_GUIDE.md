# Hugging Face Integration Submission Guide

This guide walks you through submitting VT Code as an integration to the Hugging Face hub-docs repository.

## Pre-Submission Checklist

- [ ] VT Code is actively maintained with recent commits
- [ ] Integration with Hugging Face Inference Providers is tested and working
- [ ] Documentation is clear and complete
- [ ] All setup instructions have been verified
- [ ] Code examples run without errors
- [ ] Troubleshooting section covers common issues

## Submission Steps

### 1. Fork the Repository

```bash
# Visit https://github.com/huggingface/hub-docs
# Click "Fork" button in top right
# Clone your fork locally
git clone https://github.com/YOUR_USERNAME/hub-docs.git
cd hub-docs
```

### 2. Create a Feature Branch

```bash
git checkout -b add/vtcode-integration
```

### 3. Update the Integration Index

Edit `docs/inference-providers/integrations/index.md`:

```markdown
| Tool | Description | Provider Support |
|------|-------------|------------------|
| [VT Code](./vtcode.md) | AI coding agent with semantic code analysis and 53+ specialized tools | Inference API, Dedicated Endpoints |
```

### 4. Add Your Integration Documentation

Copy the VT Code integration page to the appropriate location:

```bash
# Option A: Add to hub-docs directly
cp docs/huggingface/vtcode.md \
  hub-docs/docs/inference-providers/integrations/vtcode.md

# Option B: Reference the repository
# If your integration is in your repo, you can reference it
```

### 5. Verify Your Changes

```bash
# Navigate to hub-docs directory
cd hub-docs

# Build documentation locally (if build tools are available)
# Check for any markdown formatting issues
```

### 6. Commit Your Changes

Follow the Hugging Face contribution guidelines:

```bash
git add docs/inference-providers/integrations/

git commit -m "Add VT Code integration to Inference Providers

- Add VT Code to integrations index
- Include comprehensive setup and configuration guide
- Document supported models and features
- Add troubleshooting and resources sections"
```

### 7. Push and Create Pull Request

```bash
git push origin add/vtcode-integration
```

Then:
1. Visit your fork on GitHub
2. Click "Compare & pull request"
3. Write a clear PR title and description
4. Reference any relevant issues
5. Submit the PR

## PR Description Template

Use this template for your PR description:

```markdown
## Description

This PR adds VT Code to the Hugging Face Inference Providers integrations directory.

VT Code is an AI coding agent that integrates with Hugging Face Inference Providers to enable intelligent code analysis, generation, and automation within development workflows.

## Changes

- Added VT Code integration page with setup instructions
- Updated integrations index to include VT Code
- Included troubleshooting and resource guides

## Testing

- [x] Tested with Hugging Face Inference API
- [x] Tested with Dedicated Endpoints
- [x] Verified all code examples work
- [x] Checked documentation completeness

## Integration Details

- **Repository**: https://github.com/vinhnx/vtcode
- **Documentation**: See `docs/huggingface/vtcode.md` for full details
- **Supported Providers**: Inference API, Dedicated Endpoints
- **Maintained**: Yes - active development

## Checklist

- [x] Documentation is clear and complete
- [x] Setup instructions are accurate
- [x] Code examples are tested
- [x] Troubleshooting section is helpful
- [x] Links are valid
- [x] Markdown is properly formatted
```

## After Submission

### Review Process

1. **Hugging Face team reviews** your PR (usually within 3-7 days)
2. **Feedback and requests** for changes if needed
3. **Approval** once everything looks good
4. **Merge** into hub-docs repository

### Promotion

Once merged:
1. Share the link to your integration on social media
2. Mention it in your project README
3. Consider writing a blog post
4. Update your project documentation

## Common Issues & Solutions

### Issue: Markdown formatting errors

**Solution**: Check your markdown in an online editor before submitting

### Issue: Dead links

**Solution**: Test all links work and are accessible

### Issue: Code examples don't run

**Solution**: 
- Follow setup instructions exactly
- Test all examples locally first
- Include necessary dependencies/environment setup

### Issue: Documentation is vague

**Solution**:
- Add step-by-step instructions
- Include code examples
- Add a troubleshooting section

## File Structure Reference

The final structure in hub-docs should look like:

```
hub-docs/
├── docs/
│   ├── inference-providers/
│   │   ├── integrations/
│   │   │   ├── index.md (updated)
│   │   │   └── vtcode.md (new)
│   │   └── ...
│   └── ...
└── ...
```

## Additional Resources

- [Hugging Face Hub-Docs Contributing Guide](https://github.com/huggingface/hub-docs/blob/main/CONTRIBUTING.md)
- [Hugging Face Community Standards](https://huggingface.co/code-of-conduct)
- [GitHub Pull Request Guide](https://docs.github.com/en/pull-requests)

## Support During Submission

If you encounter issues:

1. **Check existing issues** in [hub-docs](https://github.com/huggingface/hub-docs/issues)
2. **Ask in discussions** - [HF Hub-Docs Discussions](https://github.com/huggingface/hub-docs/discussions)
3. **Contact VT Code team** - [VT Code Issues](https://github.com/vinhnx/vtcode/issues)
4. **Hugging Face Support** - [support.huggingface.co](https://support.huggingface.co)

## Success Criteria

Your integration is successfully submitted when:

✅ PR is merged into hub-docs main branch
✅ Integration appears in the integrations directory
✅ Documentation is publicly accessible
✅ Setup instructions guide users to success
