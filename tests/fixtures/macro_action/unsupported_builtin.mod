// inventory: length of an array is ordinary macro text, not an incomplete expansion
@#define n = length([1,2])
var y;
model;
y = @{n};
end;
