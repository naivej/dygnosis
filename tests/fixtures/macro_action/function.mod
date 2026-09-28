// inventory: macro function expansion; Dynare 7.2 check accepts y = 2
@#define f(x) = x+1
var y;
model;
y = @{f(1)};
end;
