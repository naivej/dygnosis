// inventory: a real macro sum evaluates before model parsing
@#define x = 0.5+0.5
var y;
model;
y = @{x};
end;
