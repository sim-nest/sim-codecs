package fixture;
public sealed class Shape permits Circle {}
final class Circle extends Shape {}
