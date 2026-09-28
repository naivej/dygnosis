// inventory: defined(X) is false when X has no macro binding
var y;
model;
@#if defined(X)
y=0;
@#else
y=1;
@#endif
end;
