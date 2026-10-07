// inventory: isempty of a non-empty array is false, so the else branch is active
var y;
model;
@#if isempty([1])
y=0;
@#else
y=1;
@#endif
end;
