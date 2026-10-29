"""Data models for the VT Code enhanced-skills demo suite.

Split out of test_enhanced.py to honor the 1000-line hard ceiling.
Pure data layer: enums and dataclasses only, no skill I/O.
"""

from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional


class SkillComplexity(Enum):
    """Skill complexity levels for progressive disclosure."""
    SIMPLE = "simple"          # Single file, basic instructions
    MODERATE = "moderate"      # Multiple files, organized structure
    COMPLEX = "complex"        # Multiple domains, extensive resources

class AccessPattern(Enum):
    """Progressive disclosure access patterns."""
    ALWAYS = "always"          # Level 1 - Always loaded (metadata only)
    ON_DEMAND = "on_demand"    # Level 2 - Loaded when skill triggered
    AS_NEEDED = "as_needed"    # Level 3 - Loaded when referenced

@dataclass
class TokenMetrics:
    """Track token usage for skills."""
    metadata_tokens: int = 0     # Level 1: Always loaded
    instruction_tokens: int = 0  # Level 2: On-demand
    resource_tokens: int = 0     # Level 3: As needed
    total_loaded: int = 0        # Actual tokens loaded
    total_available: int = 0     # If everything loaded
    efficiency_ratio: float = 0.0  # loaded / available
    
    def calculate_efficiency(self):
        """Calculate how efficiently tokens are used."""
        if self.total_available > 0:
            self.efficiency_ratio = self.total_loaded / self.total_available
        return self.efficiency_ratio

@dataclass
class SkillFile:
    """Represents a file in a skill directory."""
    path: Path
    content: str
    size_bytes: int
    access_pattern: AccessPattern
    token_count: int = 0
    last_loaded: Optional[datetime] = None
    load_count: int = 0
    
    def __post_init__(self):
        self.token_count = self.count_tokens()
        self.size_bytes = len(self.content.encode('utf-8'))
    
    def count_tokens(self) -> int:
        """Count tokens in content (simplified estimation)."""
        # Simple token count: approximately 1 token per 4 characters
        # This is a rough estimate - real Claude tokenization is more complex
        return len(self.content) // 4
    
    def should_load(self, context: Dict[str, Any]) -> bool:
        """Determine if this file should be loaded based on access pattern."""
        if self.access_pattern == AccessPattern.ALWAYS:
            return True
        elif self.access_pattern == AccessPattern.ON_DEMAND:
            return context.get('skill_triggered', False)
        elif self.access_pattern == AccessPattern.AS_NEEDED:
            return context.get('file_referenced', False)
        return False

@dataclass
class SkillInstruction:
    """Instruction with token efficiency tracking."""
    id: str
    text: str
    required: bool = True
    token_count: int = 0
    priority: int = 1
    dependencies: List[str] = field(default_factory=list)
    
    def __post_init__(self):
        self.token_count = self.count_tokens()
    
    def count_tokens(self) -> int:
        """Count tokens in instruction text."""
        return len(self.text) // 4

@dataclass 
class EnhancedSkillManifest:
    """Enhanced skill manifest with optimization metadata."""
    name: str
    description: str
    version: str
    author: Optional[str]
    category: str
    tags: List[str]
    
    # Token optimization
    complexity: SkillComplexity
    estimated_tokens: int
    critical_tokens: int  # Minimum tokens needed for basic functionality
    
    # Progressive disclosure
    access_patterns: Dict[str, AccessPattern]
    
    # Performance metrics
    load_times: List[float] = field(default_factory=list)
    success_rates: List[float] = field(default_factory=list)
    token_metrics: TokenMetrics = field(default_factory=TokenMetrics)
    
    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for serialization."""
        return {
            'name': self.name,
            'description': self.description,
            'version': self.version,
            'author': self.author,
            'category': self.category,
            'tags': self.tags,
            'complexity': self.complexity.value,
            'estimated_tokens': self.estimated_tokens,
            'critical_tokens': self.critical_tokens,
            'token_efficiency': self.token_metrics.efficiency_ratio,
            'avg_load_time': sum(self.load_times) / len(self.load_times) if self.load_times else 0
        }
