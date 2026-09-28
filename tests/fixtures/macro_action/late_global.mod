// inventory: function body syntax is checked before its free name is evaluated
@#define f(x) = x+g
@#define g = 1
var y;
model;
y = @{f(1)};
end;
