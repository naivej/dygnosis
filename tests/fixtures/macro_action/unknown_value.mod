// inventory: Dynare's macro stage refuses the bare undefined value
@#define bad = zz
var y;
model;
y = @{bad};
end;
