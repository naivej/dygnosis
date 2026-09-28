// inventory: a pinned builtin beyond this evaluator remains valid and incomplete
var y;
model;
@#if isempty([1])
y=0;
@#else
y=1;
@#endif
end;
